//! 推理能力发现编排层（Step 3）。
//!
//! 职责：调度 Adapter、按 Tier 0 → Tier 1 → Tier 2 顺序取证、合并证据、产出最终
//! ReasoningCapability。本模块不做任何 `if provider == ...` 判断——协议差异全部由
//! Adapter 通过自描述的探测目标（含 AuthScheme）表达。
//!
//! 三条硬约束：
//! 1. 永远不返回 Err。能力探测失败绝不能阻塞模型保存，所有网络错误在内部吞掉。
//! 2. 严格不进入 Tier 3（计费真实请求）。validation probe 若被网关放行返回 2xx，
//!    视为"无法确认"而非"支持"，并记入 note 供上层提示。
//! 3. 缓存归属 (base_url, model_id)。ReasoningCapability::merge 会校验 key，
//!    换 base_url 后旧能力不可能被误用。

use reqwest::{Client, RequestBuilder};
use serde_json::Value;
use url::Url;

use crate::{
    model::ProtocolKind,
    reasoning_adapters::{
        adapter_for, capability_from_metadata, capability_from_validation, enforce_output_limits,
        AuthScheme, ErrorInterpretation, MetadataHints, ReasoningAdapter,
        CAPABILITY_VALIDATION_PROBE, PROBE_HEADER,
    },
    reasoning_capability::{
        EvidenceSource, ReasoningCapability, ReasoningControl, ReasoningEvidence, ReasoningKey,
        ReasoningSupport, TTL_SUPPORTED_SECONDS, TTL_UNKNOWN_SECONDS, TTL_UNSUPPORTED_SECONDS,
    },
};

const ANTHROPIC_VERSION: &str = "2023-06-01";

/// Tier 0 元数据的来源状态。让编排层知道"是否已经有人取过 /models/{id}"，
/// 避免与现有 context_window 流程重复发请求。
#[derive(Debug, Clone, Copy)]
pub enum MetadataSource<'a> {
    /// 调用方已拿到模型详情响应体，直接复用（零额外请求）。
    Provided(&'a Value),
    /// 调用方已尝试过同一端点但失败了，不要重试。
    Attempted,
    /// 调用方没有取过，允许本模块按 adapter.metadata_target() 自行获取。
    Absent,
}

/// 一次发现的产出。capability 永远有值：最坏情况是"沿用旧缓存"或"Unknown"。
#[derive(Debug, Clone)]
pub struct DiscoveryOutcome {
    pub capability: ReasoningCapability,
    /// 本次实际访问过的端点，供上层并入 ProbeResult.checked_endpoints。
    pub checked_endpoints: Vec<String>,
    /// 面向用户的中文说明，仅在发生了值得提示的情况时给出。
    pub note: Option<String>,
    /// 结论是否相对入参缓存发生了变化。false 表示上层无需改写已存能力。
    pub changed: bool,
}

/// 该协议是否参与推理能力发现。Azure 无法枚举部署，缺少可靠的元数据端点，本阶段排除。
pub fn supports_discovery(protocol: ProtocolKind) -> bool {
    matches!(protocol, ProtocolKind::Openai | ProtocolKind::Anthropic | ProtocolKind::Gemini)
}

/// 判断 adapter 的 Tier 0 端点与调用方已取的详情端点是否指向同一处。
/// 用于让上层复用已有响应体，而不必在 protocol.rs 里写协议分支。
pub fn metadata_endpoint_matches(base_url: &str, protocol: ProtocolKind, model_id: &str, candidate: &str) -> bool {
    let adapter = adapter_for(protocol);
    let Some(target) = adapter.metadata_target(model_id) else { return false };
    match join_endpoint(base_url, &target.endpoint) {
        Some(resolved) => resolved == candidate,
        None => false,
    }
}

/// 发现 (base_url, model_id) 的推理能力。
///
/// - `cached`：该模型此前已存的能力，用于 TTL 短路与 merge 基线。
/// - `metadata`：Tier 0 数据来源，见 [`MetadataSource`]。
pub async fn discover_reasoning_capability(
    client: &Client,
    protocol: ProtocolKind,
    base_url: &str,
    model_id: &str,
    api_key: &str,
    metadata: MetadataSource<'_>,
    cached: Option<&ReasoningCapability>,
) -> DiscoveryOutcome {
    let key = ReasoningKey::new(base_url, model_id);

    // 缓存短路：键一致且未过期且已有结论 → 零请求返回。
    if let Some(existing) = cached {
        if existing.key == key && !existing.should_rediscover() {
            return DiscoveryOutcome {
                capability: existing.clone(),
                checked_endpoints: Vec::new(),
                note: None,
                changed: false,
            };
        }
    }

    // 基线：键一致的旧能力才可作为基线，否则从 Unknown 起步（换 base_url 不继承）。
    let baseline = cached
        .filter(|existing| existing.key == key)
        .cloned()
        .unwrap_or_else(|| ReasoningCapability::unknown(key.clone()));

    let adapter = adapter_for(protocol);
    let mut state = DiscoveryState {
        capability: baseline.clone(),
        checked_endpoints: Vec::new(),
        notes: Vec::new(),
        got_conclusion: false,
        probe_completed: false,
    };

    run_tier0(&mut state, client, adapter.as_ref(), base_url, model_id, api_key, metadata, &key).await;

    // 严格的回退阶梯：上一级拿到结论就不再往下花请求。
    // 因此"元数据自述完整的 Provider"整条链路是零额外请求。
    if !state.got_conclusion {
        run_tier1(&mut state, client, adapter.as_ref(), base_url, model_id, api_key, &key).await;
    }
    if !state.got_conclusion {
        run_tier2(&mut state, client, adapter.as_ref(), base_url, model_id, api_key, &key).await;
    }

    // 只要本轮真的得到过服务端的答复（有结论，或 Tier 2 完成了探测），就重置计时窗口。
    // 这是"每个 (base_url, model_id) 每个 TTL 周期最多探测一次"的唯一强制点：
    // 未探明同样是需要缓存的结论，否则 Unknown 会在每次 save_provider 时重发探测。
    let settled = state.got_conclusion || state.probe_completed;
    if settled {
        state.capability.discovered_at = chrono::Utc::now().to_rfc3339();
        state.capability.ttl_seconds = retry_window(&state.capability, state.got_conclusion);
    }

    // 首次发现必须落盘，哪怕结论是 Unknown——否则盘上没有记录，
    // 下次保存无缓存可短路，退避窗口形同虚设。
    let changed = state.capability != baseline || (cached.is_none() && settled);
    DiscoveryOutcome {
        capability: state.capability,
        checked_endpoints: state.checked_endpoints,
        note: if state.notes.is_empty() { None } else { Some(state.notes.join(" ")) },
        changed,
    }
}

/// 本轮结论对应的缓存时长。
///
/// 没得出结论时一律用 Unknown 窗口（6 小时）作为退避，即使 capability 里仍带着
/// 上次的 Supported 结论——那个结论已经过期了，用 14 天续期它等于让一次限流
/// 顶掉半个月的重新验证。
fn retry_window(capability: &ReasoningCapability, got_conclusion: bool) -> u64 {
    if !got_conclusion { return TTL_UNKNOWN_SECONDS; }
    match capability.support {
        ReasoningSupport::Supported => TTL_SUPPORTED_SECONDS,
        ReasoningSupport::Unsupported => TTL_UNSUPPORTED_SECONDS,
        ReasoningSupport::Unknown => TTL_UNKNOWN_SECONDS,
    }
}

struct DiscoveryState {
    capability: ReasoningCapability,
    checked_endpoints: Vec<String>,
    notes: Vec<String>,
    /// 是否已经得到可用结论，决定是否继续往下花请求。
    got_conclusion: bool,
    /// Tier 2 是否真的拿到了服务端响应（而非网络失败）。
    /// 只有"探测确实完成"才重置 TTL 窗口，否则一次网络抖动会白白锁住 6 小时。
    probe_completed: bool,
}

impl DiscoveryState {
    fn mark(&mut self, endpoint: &str) {
        if !self.checked_endpoints.iter().any(|item| item == endpoint) {
            self.checked_endpoints.push(endpoint.to_owned());
        }
    }

    fn note(&mut self, text: impl Into<String>) {
        let text = text.into();
        if !self.notes.iter().any(|item| item == &text) { self.notes.push(text); }
    }

    /// 记一条 Tier 2 证据。用于"探测执行了但没得出结论"的三种情况——
    /// 放行、限流/5xx、错误消息无信息。有结论的情况由 capability_from_validation 负责。
    fn record(&mut self, endpoint: &str, detail: impl Into<String>) {
        self.capability.push_evidence(ReasoningEvidence::new(
            EvidenceSource::CapabilityValidation,
            Some(endpoint.to_owned()),
            detail,
        ));
    }
}

/// Tier 0：模型元数据声明。优先复用调用方已取的响应体，零额外请求。
async fn run_tier0(
    state: &mut DiscoveryState,
    client: &Client,
    adapter: &dyn ReasoningAdapter,
    base_url: &str,
    model_id: &str,
    api_key: &str,
    metadata: MetadataSource<'_>,
    key: &ReasoningKey,
) {
    let body = match metadata {
        MetadataSource::Provided(body) => Some(body.clone()),
        // 调用方已经试过并失败，不重复消耗请求。
        MetadataSource::Attempted => None,
        MetadataSource::Absent => {
            let Some(target) = adapter.metadata_target(model_id) else { return };
            let Some(resolved) = join_endpoint(base_url, &target.endpoint) else { return };
            state.mark(&resolved);
            match fetch_json(client.get(&resolved), target.auth, api_key).await {
                ProbeResponse::Json { status, body } if (200..300).contains(&status) => Some(body),
                ProbeResponse::Json { .. } | ProbeResponse::Opaque { .. } => None,
                ProbeResponse::Transport => {
                    state.note("模型元数据端点不可达，已保留既有推理能力结论。");
                    return;
                }
            }
        }
    };

    let Some(body) = body else { return };
    let hints = extract_hints(adapter, &body);
    let Some(incoming) = capability_from_metadata(key.clone(), hints) else { return };
    if state.capability.merge(incoming) {
        state.got_conclusion = true;
    }
}

/// Tier 1：introspection 端点。仅在 Tier 0 没有结论时执行。
async fn run_tier1(
    state: &mut DiscoveryState,
    client: &Client,
    adapter: &dyn ReasoningAdapter,
    base_url: &str,
    model_id: &str,
    api_key: &str,
    key: &ReasoningKey,
) {
    for target in adapter.introspection_targets(model_id) {
        let Some(resolved) = join_endpoint(base_url, &target.endpoint) else { continue };
        let request = if target.method.eq_ignore_ascii_case("POST") {
            client.post(&resolved)
        } else {
            client.get(&resolved)
        };
        state.mark(&resolved);
        let body = match fetch_json(request, target.auth, api_key).await {
            ProbeResponse::Json { status, body } if (200..300).contains(&status) => body,
            ProbeResponse::Json { .. } | ProbeResponse::Opaque { .. } => continue,
            ProbeResponse::Transport => {
                state.note("能力查询端点不可达，已保留既有推理能力结论。");
                continue;
            }
        };

        // extract_path 声明了该端点值得关注的字段；一个都不存在时说明这个端点没带能力信息。
        if !target.extract_path.is_empty()
            && !target.extract_path.iter().any(|path| body.get(path).is_some())
        {
            continue;
        }

        let hints = extract_hints(adapter, &body);
        let Some(mut incoming) = capability_from_metadata(key.clone(), hints) else { continue };
        // 证据来源改记为 Introspection，并带上端点，便于 UI 展示取证路径。
        relabel_evidence(&mut incoming, EvidenceSource::Introspection, &resolved);
        if state.capability.merge(incoming) {
            state.got_conclusion = true;
            return;
        }
    }
}

/// Tier 2：capability validation probe。发送故意越界的推理参数，从 400/422 错误消息里读出真相。
///
/// 这是本模块唯一会打到真实推理端点的一步，属于产品定位允许的小成本能力验证。三条约束：
/// 1. 带 `x-provider-deck-probe: capability-validation` 自识别头；
/// 2. 输出上限由本函数强制压到 1 token，Adapter 写的值一律被覆写；
/// 3. 无论结论如何都写入 evidence，并由调用方统一落进缓存 TTL。
async fn run_tier2(
    state: &mut DiscoveryState,
    client: &Client,
    adapter: &dyn ReasoningAdapter,
    base_url: &str,
    model_id: &str,
    api_key: &str,
    key: &ReasoningKey,
) {
    let Some(mut probe) = adapter.validation_probe(model_id) else { return };
    let Some(resolved) = join_endpoint(base_url, &probe.endpoint) else { return };

    // 成本闸门：一个输出上限都写不进去，就不发这个请求。
    // 宁可结论 Unknown，也不发一个没有输出上限的推理请求。
    if enforce_output_limits(&mut probe.body, &probe.output_limits) == 0 {
        state.note("该协议未声明可用的输出上限字段，已跳过能力验证探测以避免不可控的输出成本。");
        return;
    }

    state.mark(&resolved);
    let request = client
        .post(&resolved)
        .header(PROBE_HEADER, CAPABILITY_VALIDATION_PROBE)
        .json(&probe.body);

    let (status, body) = match fetch_json(request, probe.auth, api_key).await {
        ProbeResponse::Json { status, body } => (status, body),
        ProbeResponse::Opaque { status } => (status, Value::Null),
        ProbeResponse::Transport => {
            // 网络失败：保留旧能力，不降级、不写入 Unknown。
            // 也不算"探测完成"，因此不重置 TTL 窗口——下次保存会重试。
            state.note("能力验证探测网络失败，已保留既有推理能力结论。");
            return;
        }
    };

    // 拿到了 HTTP 响应即视为探测完成：无论结论如何都要入缓存，
    // 这是"Unknown 不再每次保存重发"的前提。
    state.probe_completed = true;

    if (200..300).contains(&status) {
        // 网关放行了越界参数。既可能是"宽松网关忽略未知字段"，也可能是真的接受了，
        // 二者无法区分，因此不下结论——但必须留下证据，否则用户只会看到一片空白，
        // 不知道这次探测已经花掉了一次请求。
        state.record(
            &resolved,
            format!("能力验证探测被服务端放行（HTTP {status}），无法据此判断推理能力。"),
        );
        state.note("能力验证探测被服务端直接放行，无法据此判断推理能力；后续可由用户主动发起真实请求验证。");
        return;
    }

    if status == 429 || status >= 500 {
        // 限流/服务端故障不携带能力信息，绝不能覆盖已有结论，但同样留证据 + 入缓存退避。
        let detail = if status == 429 {
            "能力验证探测被限流（HTTP 429），推理能力暂未探明。".to_owned()
        } else {
            format!("能力验证探测遇到服务端错误（HTTP {status}），推理能力暂未探明。")
        };
        state.record(&resolved, detail.clone());
        state.note(detail);
        return;
    }

    match adapter.interpret_error(status, &body) {
        ErrorInterpretation::Unknown => {
            state.record(
                &resolved,
                format!("能力验证探测返回 HTTP {status}，但错误消息未包含能力信息。"),
            );
            state.note("能力验证错误消息未包含能力信息，推理能力暂未探明。");
        }
        interpretation => {
            if let Ok(incoming) = capability_from_validation(key.clone(), interpretation, &resolved) {
                absorb_validation(&mut state.capability, incoming);
                state.got_conclusion = true;
            }
        }
    }
}

/// 把 Tier 2 结论并入现状。
///
/// 直接 merge 有个陷阱：Tier 2 的 `Supported` 只证明"该参数存在"，tiers 是空的，
/// 但 confidence 更高，merge 会用空档位覆盖 Tier 0 已经拿到的档位表。这里对这种情况
/// 只提升置信度并累积证据，保留更富信息的 control/tiers。
fn absorb_validation(current: &mut ReasoningCapability, incoming: ReasoningCapability) {
    // Unsupported 是明确结论，不是"信息量不足"：它必须顶掉旧档位表，否则会出现
    // "服务端说不支持，UI 还在显示档位"。只有 Supported 才走下面的保守合并。
    let incoming_is_bare = incoming.support != ReasoningSupport::Unsupported
        && incoming.tiers.is_empty()
        && matches!(incoming.control, ReasoningControl::None);
    let current_has_detail = !current.tiers.is_empty();

    if incoming_is_bare && current_has_detail {
        if incoming.confidence > current.confidence { current.confidence = incoming.confidence; }
        for evidence in incoming.evidence { current.push_evidence(evidence); }
        return;
    }

    current.merge(incoming);
}

/// 复用 Adapter 的 metadata_hints，但把"完全空线索"归一化掉。
///
/// 某些 Adapter 会无条件填协议常量（如 Gemini 的 dynamic_sentinel = -1），
/// 这类字段本身不构成证据；capability_from_metadata 只看 fields/effort/budget，
/// 因此这里原样透传即可，函数存在的意义是给 Tier 0/Tier 1 共用一个入口。
fn extract_hints(adapter: &dyn ReasoningAdapter, body: &Value) -> MetadataHints {
    adapter.metadata_hints(body)
}

/// 将 capability 内的元数据类证据改标为指定来源，并补上端点。
fn relabel_evidence(capability: &mut ReasoningCapability, source: EvidenceSource, endpoint: &str) {
    let original = std::mem::take(&mut capability.evidence);
    for item in original {
        let mut moved = ReasoningEvidence::new(source, Some(endpoint.to_owned()), item.detail);
        moved.observed_at = item.observed_at;
        capability.push_evidence(moved);
    }
}

/// 传输层结果。区分"拿到了 JSON"、"拿到了响应但不是 JSON"、"根本没连上"，
/// 因为三者对应完全不同的能力结论（可判断 / 不可判断 / 保留旧值）。
enum ProbeResponse {
    Json { status: u16, body: Value },
    Opaque { status: u16 },
    Transport,
}

async fn fetch_json(request: RequestBuilder, auth: AuthScheme, api_key: &str) -> ProbeResponse {
    let request = apply_auth(request, auth, api_key);
    let Ok(response) = request.send().await else { return ProbeResponse::Transport };
    let status = response.status().as_u16();
    match response.json::<Value>().await {
        Ok(body) => ProbeResponse::Json { status, body },
        Err(_) => ProbeResponse::Opaque { status },
    }
}

/// 按 Adapter 自述的鉴权方式加头。这是编排层唯一需要知道的"协议差异"，
/// 且它由 Adapter 声明而非在此判断协议类型。
fn apply_auth(request: RequestBuilder, auth: AuthScheme, api_key: &str) -> RequestBuilder {
    match auth {
        AuthScheme::Bearer => request.header(reqwest::header::AUTHORIZATION, format!("Bearer {api_key}")),
        AuthScheme::AnthropicKey => request
            .header("x-api-key", api_key)
            .header("anthropic-version", ANTHROPIC_VERSION),
        AuthScheme::GoogleKey => request.header("x-goog-api-key", api_key),
    }
}

/// 把 Adapter 给出的绝对路径拼到已归一化的 base_url 上。
///
/// 两个要点：
/// - base_url 自带路径前缀时不重复叠加（`.../v1` + `/v1/models/x` → `.../v1/models/x`）；
/// - 路径里可能含 `:generateContent` 这种冒号段，必须走 set_path 而非 path_segments_mut，
///   否则冒号会被百分号编码。
fn join_endpoint(base_url: &str, path: &str) -> Option<String> {
    let mut url = Url::parse(base_url).ok()?;
    let base_path = url.path().trim_end_matches('/').to_owned();
    let suffix = format!("/{}", path.trim_start_matches('/'));

    let combined = if base_path.is_empty() || base_path == "/" {
        suffix
    } else if let Some(rest) = overlap_suffix(&base_path, &suffix) {
        format!("{base_path}{rest}")
    } else {
        format!("{base_path}{suffix}")
    };

    url.set_path(&combined);
    url.set_query(None);
    url.set_fragment(None);
    Some(url.to_string())
}

/// base_path 末尾与 suffix 开头重叠时，返回 suffix 去掉重叠部分后的剩余段。
/// 例：base `/v1`、suffix `/v1/models/x` → `/models/x`。
fn overlap_suffix(base_path: &str, suffix: &str) -> Option<String> {
    let base_segments: Vec<&str> = base_path.trim_matches('/').split('/').filter(|s| !s.is_empty()).collect();
    let suffix_segments: Vec<&str> = suffix.trim_matches('/').split('/').filter(|s| !s.is_empty()).collect();
    if base_segments.is_empty() || suffix_segments.is_empty() { return None; }

    let max_overlap = base_segments.len().min(suffix_segments.len());
    for take in (1..=max_overlap).rev() {
        let base_tail = &base_segments[base_segments.len() - take..];
        let suffix_head = &suffix_segments[..take];
        if base_tail == suffix_head {
            let rest = &suffix_segments[take..];
            if rest.is_empty() { return Some(String::new()); }
            return Some(format!("/{}", rest.join("/")));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reasoning_capability::ReasoningConfidence;
    use std::{
        io::{Read, Write},
        net::TcpListener,
        sync::{Arc, Mutex},
        thread,
    };

    type Recorded = Arc<Mutex<Vec<String>>>;

    /// 按路径片段路由的 mock server：(路径包含的片段, status, body)。首个命中者生效。
    /// 沿用工程既有的手写 TcpListener 约定，不引入额外 mock 依赖。
    fn mock_server(routes: Vec<(&'static str, u16, &'static str)>) -> (String, Recorded) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let recorded: Recorded = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&recorded);
        thread::spawn(move || {
            // listener 随线程存活；测试结束进程回收，无需显式 join。
            while let Ok((mut stream, _)) = listener.accept() {
                let mut request = Vec::new();
                let mut chunk = [0_u8; 4096];
                loop {
                    let Ok(read) = stream.read(&mut chunk) else { break };
                    if read == 0 { break; }
                    request.extend_from_slice(&chunk[..read]);
                    let Some(header_end) = request.windows(4).position(|window| window == b"\r\n\r\n") else { continue };
                    let headers = String::from_utf8_lossy(&request[..header_end]);
                    let content_length = headers
                        .lines()
                        .find_map(|line| {
                            let (name, value) = line.split_once(':')?;
                            name.eq_ignore_ascii_case("content-length")
                                .then(|| value.trim().parse::<usize>().ok())
                                .flatten()
                        })
                        .unwrap_or(0);
                    if request.len() >= header_end + 4 + content_length { break; }
                }
                let request = String::from_utf8_lossy(&request).to_string();
                sink.lock().unwrap().push(request.clone());
                let path = request.lines().next().unwrap_or("").to_owned();
                let (status, body) = routes
                    .iter()
                    .find(|(route, _, _)| path.contains(route))
                    .map(|(_, status, body)| (*status, *body))
                    .unwrap_or((404, "{}"));
                let reason = if (200..300).contains(&status) { "OK" } else { "Error" };
                let _ = write!(
                    stream,
                    "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
            }
        });
        (format!("http://{address}/v1"), recorded)
    }

    /// 一个已绑定又立即释放的地址：连接必然失败，用于模拟网络不可达。
    fn dead_base_url() -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        drop(listener);
        format!("http://{address}/v1")
    }

    /// 测试专用客户端：显式绕过系统代理。
    ///
    /// 开发机上若配了系统代理，连 127.0.0.1 也会被它接管——目标端口没人监听时代理
    /// 返回 502 而不是连接失败，"网络不可达"分支就永远测不到。断言必须只取决于被测
    /// 代码，不取决于跑测试的机器。
    fn test_client() -> Client {
        Client::builder().no_proxy().build().expect("构建测试客户端失败")
    }

    fn stale(mut capability: ReasoningCapability) -> ReasoningCapability {
        capability.discovered_at = (chrono::Utc::now() - chrono::Duration::days(60)).to_rfc3339();
        capability
    }

    fn supported_cache(base_url: &str, model_id: &str) -> ReasoningCapability {
        ReasoningCapability::from_effort_enum(
            ReasoningKey::new(base_url, model_id),
            &["low".into(), "medium".into(), "high".into()],
            ReasoningConfidence::Declared,
        )
    }

    #[test]
    fn joins_endpoints_without_duplicating_known_prefix() {
        assert_eq!(
            join_endpoint("https://api.example.com/v1", "/v1/models/gpt-x").unwrap(),
            "https://api.example.com/v1/models/gpt-x"
        );
        assert_eq!(
            join_endpoint("https://api.example.com", "/v1/models/gpt-x").unwrap(),
            "https://api.example.com/v1/models/gpt-x"
        );
        assert_eq!(
            join_endpoint("https://gw.example.com/proxy/openai/v1", "/v1/chat/completions").unwrap(),
            "https://gw.example.com/proxy/openai/v1/chat/completions"
        );
    }

    /// Gemini 的 `:generateContent` 冒号段不能被百分号编码。
    #[test]
    fn preserves_colon_segments() {
        assert_eq!(
            join_endpoint("https://generativelanguage.googleapis.com", "/v1beta/models/gemini-x:generateContent").unwrap(),
            "https://generativelanguage.googleapis.com/v1beta/models/gemini-x:generateContent"
        );
    }

    /// 要求 4：已存在未过期缓存 → 不发送任何网络请求。
    #[tokio::test]
    async fn unexpired_cache_sends_no_request() {
        let (base_url, recorded) = mock_server(vec![]);
        let cached = supported_cache(&base_url, "gpt-x");
        let outcome = discover_reasoning_capability(
            &test_client(),
            ProtocolKind::Openai,
            &base_url,
            "gpt-x",
            "key",
            MetadataSource::Absent,
            Some(&cached),
        )
        .await;

        assert!(recorded.lock().unwrap().is_empty(), "缓存未过期却发起了请求");
        assert!(outcome.checked_endpoints.is_empty());
        assert!(!outcome.changed);
        assert_eq!(outcome.capability.support, ReasoningSupport::Supported);
    }

    /// 要求 7：同 model_id、不同 base_url 不能共享 capability。
    #[tokio::test]
    async fn same_model_id_across_base_urls_does_not_share_capability() {
        let (base_url, _recorded) = mock_server(vec![("/v1/models/gpt-x", 404, "{}")]);
        // 缓存属于另一个 base_url，键不匹配，必须被忽略而不是继承。
        let foreign = supported_cache("https://other.example.com/v1", "gpt-x");
        let outcome = discover_reasoning_capability(
            &test_client(),
            ProtocolKind::Openai,
            &base_url,
            "gpt-x",
            "key",
            MetadataSource::Absent,
            Some(&foreign),
        )
        .await;

        assert_eq!(outcome.capability.key, ReasoningKey::new(&base_url, "gpt-x"));
        assert_ne!(outcome.capability.support, ReasoningSupport::Supported);
        assert!(outcome.capability.tiers.is_empty(), "跨 base_url 继承了旧档位");
    }

    /// 要求 1：OpenAI 模型触发 discovery。元数据自述完整时零额外请求。
    #[tokio::test]
    async fn openai_metadata_declaration_needs_no_extra_request() {
        let (base_url, recorded) = mock_server(vec![]);
        let body = serde_json::json!({
            "id": "gpt-x",
            "capabilities": { "reasoning": { "effort": ["low", "medium", "high"] } }
        });
        let outcome = discover_reasoning_capability(
            &test_client(),
            ProtocolKind::Openai,
            &base_url,
            "gpt-x",
            "key",
            MetadataSource::Provided(&body),
            None,
        )
        .await;

        assert!(recorded.lock().unwrap().is_empty(), "Tier 0 命中却仍发起了请求");
        assert_eq!(outcome.capability.support, ReasoningSupport::Supported);
        assert_eq!(outcome.capability.confidence, ReasoningConfidence::Declared);
        assert!(!outcome.capability.tiers.is_empty());
        assert!(outcome.changed);
    }

    /// 要求 2：Anthropic 模型触发 discovery，且使用 x-api-key 而非 Bearer。
    #[tokio::test]
    async fn anthropic_discovery_uses_its_own_auth_scheme() {
        let (base_url, recorded) = mock_server(vec![(
            "/v1/models/claude-x",
            200,
            r#"{"id":"claude-x","capabilities":{"thinking":true},"thinking":{"budget_min":1024,"budget_max":32000}}"#,
        )]);
        let outcome = discover_reasoning_capability(
            &test_client(),
            ProtocolKind::Anthropic,
            &base_url,
            "claude-x",
            "secret-key",
            MetadataSource::Absent,
            None,
        )
        .await;

        let requests = recorded.lock().unwrap().clone();
        assert_eq!(requests.len(), 1, "Tier 0 命中后不应继续往下探测");
        let lower = requests[0].to_lowercase();
        assert!(lower.contains("x-api-key: secret-key"), "缺少 Anthropic 鉴权头：{}", requests[0]);
        assert!(lower.contains("anthropic-version"), "缺少 anthropic-version 头");
        assert!(!lower.contains("authorization: bearer"), "错误地使用了 Bearer 鉴权");
        assert_eq!(outcome.capability.support, ReasoningSupport::Supported);
    }

    /// 要求 3：Gemini 模型触发 discovery，走 /v1beta 元数据端点。
    #[tokio::test]
    async fn gemini_discovery_hits_v1beta_metadata_endpoint() {
        let (base_url, recorded) = mock_server(vec![(
            "/v1beta/models/gemini-x",
            200,
            r#"{"name":"models/gemini-x","thinkingConfig":{"thinkingBudgetMin":0,"thinkingBudgetMax":24576}}"#,
        )]);
        let outcome = discover_reasoning_capability(
            &test_client(),
            ProtocolKind::Gemini,
            &base_url,
            "models/gemini-x",
            "google-key",
            MetadataSource::Absent,
            None,
        )
        .await;

        let requests = recorded.lock().unwrap().clone();
        assert_eq!(requests.len(), 1);
        assert!(requests[0].contains("/v1beta/models/gemini-x"), "未使用 Gemini 的 v1beta 端点：{}", requests[0]);
        assert!(requests[0].to_lowercase().contains("x-goog-api-key: google-key"));
        assert_eq!(outcome.capability.support, ReasoningSupport::Supported);
        assert!(outcome.checked_endpoints.iter().any(|item| item.contains("/v1beta/models/gemini-x")));
    }

    /// 要求 5：网络失败必须保留旧 ReasoningCapability。
    #[tokio::test]
    async fn network_failure_keeps_previous_capability() {
        let base_url = dead_base_url();
        let cached = stale(supported_cache(&base_url, "gpt-x"));
        let outcome = discover_reasoning_capability(
            &test_client(),
            ProtocolKind::Openai,
            &base_url,
            "gpt-x",
            "key",
            MetadataSource::Absent,
            Some(&cached),
        )
        .await;

        assert_eq!(outcome.capability.support, ReasoningSupport::Supported);
        assert_eq!(outcome.capability.tiers.len(), cached.tiers.len());
        assert!(!outcome.changed, "网络失败却改写了结论");
        assert!(outcome.note.is_some(), "网络失败应给出说明");
        // 网络失败不算"探测完成"：不重置计时窗口，下次保存仍会重试。
        assert_eq!(outcome.capability.ttl_seconds, cached.ttl_seconds);
        assert!(outcome.capability.evidence.iter().all(|item| item.source != EvidenceSource::CapabilityValidation));
    }

    /// 要求 6：429 不能覆盖已有能力的结论，但要退避（6 小时）并留证据。
    #[tokio::test]
    async fn rate_limit_cannot_overwrite_existing_capability() {
        let (base_url, _recorded) = mock_server(vec![("", 429, r#"{"error":{"message":"rate limit exceeded"}}"#)]);
        let cached = stale(supported_cache(&base_url, "gpt-x"));
        let outcome = discover_reasoning_capability(
            &test_client(),
            ProtocolKind::Openai,
            &base_url,
            "gpt-x",
            "key",
            MetadataSource::Attempted,
            Some(&cached),
        )
        .await;

        assert_eq!(outcome.capability.support, ReasoningSupport::Supported);
        assert_eq!(outcome.capability.confidence, ReasoningConfidence::Declared);
        assert!(outcome.note.as_deref().unwrap_or_default().contains("429"));
        // 结论保留，但计时窗口按"未得出结论"退避：一次限流不该续期半个月。
        assert_eq!(outcome.capability.ttl_seconds, TTL_UNKNOWN_SECONDS);
        assert!(outcome.changed, "退避窗口必须落盘，否则下次保存会立刻重发探测");
        assert!(outcome
            .capability
            .evidence
            .iter()
            .any(|item| item.source == EvidenceSource::CapabilityValidation && item.detail.contains("429")));
    }

    /// 要求 6：5xx 同样不能覆盖已有能力的结论。
    #[tokio::test]
    async fn server_error_cannot_overwrite_existing_capability() {
        let (base_url, _recorded) = mock_server(vec![("", 500, r#"{"error":{"message":"internal"}}"#)]);
        let cached = stale(supported_cache(&base_url, "gpt-x"));
        let outcome = discover_reasoning_capability(
            &test_client(),
            ProtocolKind::Openai,
            &base_url,
            "gpt-x",
            "key",
            MetadataSource::Attempted,
            Some(&cached),
        )
        .await;

        assert_eq!(outcome.capability.support, ReasoningSupport::Supported);
        assert_eq!(outcome.capability.tiers.len(), cached.tiers.len());
        assert_eq!(outcome.capability.ttl_seconds, TTL_UNKNOWN_SECONDS);
    }

    /// Tier 2 明确否认时下调为 Unsupported。
    #[tokio::test]
    async fn validation_probe_can_conclude_unsupported() {
        let (base_url, _recorded) = mock_server(vec![(
            "/v1/chat/completions",
            400,
            r#"{"error":{"message":"Unrecognized request argument supplied: reasoning"}}"#,
        )]);
        let outcome = discover_reasoning_capability(
            &test_client(),
            ProtocolKind::Openai,
            &base_url,
            "gpt-x",
            "key",
            MetadataSource::Attempted,
            None,
        )
        .await;

        assert_eq!(outcome.capability.support, ReasoningSupport::Unsupported);
        assert_eq!(outcome.capability.confidence, ReasoningConfidence::Validated);
        assert!(outcome.changed);
    }

    /// Tier 2 的 `Supported` 只证明"参数存在"，不得用空档位覆盖已有档位表。
    #[tokio::test]
    async fn validation_probe_raises_confidence_without_erasing_tiers() {
        let (base_url, _recorded) = mock_server(vec![(
            "/v1/chat/completions",
            400,
            r#"{"error":{"message":"Invalid value: 'invalid_effort_value_for_validation'. Supported values are: 'low', 'medium', 'high'."}}"#,
        )]);
        let cached = stale(supported_cache(&base_url, "gpt-x"));
        let outcome = discover_reasoning_capability(
            &test_client(),
            ProtocolKind::Openai,
            &base_url,
            "gpt-x",
            "key",
            MetadataSource::Attempted,
            Some(&cached),
        )
        .await;

        assert_eq!(outcome.capability.confidence, ReasoningConfidence::Validated);
        assert_eq!(outcome.capability.support, ReasoningSupport::Supported);
        assert_eq!(outcome.capability.tiers.len(), cached.tiers.len(), "Tier 2 抹掉了 Tier 0 的档位表");
    }

    /// 要求 4：越界参数被网关放行时不下结论，但必须写入 evidence 并进入缓存。
    /// 放行无法区分"宽松网关忽略了未知字段"和"真的接受了"，确证留给后续 Step 的
    /// 用户主动验证；但这次请求确实花掉了，用户有权看到。
    #[tokio::test]
    async fn accepted_validation_probe_records_evidence_and_caches() {
        let (base_url, recorded) = mock_server(vec![("/v1/chat/completions", 200, r#"{"id":"chatcmpl-1"}"#)]);
        let outcome = discover_reasoning_capability(
            &test_client(),
            ProtocolKind::Openai,
            &base_url,
            "gpt-x",
            "key",
            MetadataSource::Attempted,
            None,
        )
        .await;

        assert_eq!(outcome.capability.support, ReasoningSupport::Unknown);
        assert_eq!(outcome.capability.confidence, ReasoningConfidence::Unknown);
        assert!(outcome.note.as_deref().unwrap_or_default().contains("放行"));

        let evidence = outcome
            .capability
            .evidence
            .iter()
            .find(|item| item.source == EvidenceSource::CapabilityValidation)
            .expect("放行未写入 CapabilityValidation evidence");
        assert!(evidence.detail.contains("200"), "evidence 未记录状态码：{}", evidence.detail);
        assert!(evidence.endpoint.as_deref().is_some_and(|item| item.contains("/v1/chat/completions")));

        // 要求 3/6：Unknown 也必须落盘并带 6 小时窗口，否则每次保存都会重发。
        assert!(outcome.changed, "首次 Unknown 结论必须落盘");
        assert_eq!(outcome.capability.ttl_seconds, TTL_UNKNOWN_SECONDS);
        assert!(!outcome.capability.should_rediscover(), "Unknown 仍被判定为需要立刻重新探测");

        // 把上一轮结论当缓存再跑一次：零请求。
        let before = recorded.lock().unwrap().len();
        let again = discover_reasoning_capability(
            &test_client(),
            ProtocolKind::Openai,
            &base_url,
            "gpt-x",
            "key",
            MetadataSource::Attempted,
            Some(&outcome.capability),
        )
        .await;
        assert_eq!(recorded.lock().unwrap().len(), before, "200 放行后仍在重复探测");
        assert!(!again.changed);
    }

    /// 要求 3：Unknown 在 6 小时窗口内零请求，窗口过后重新探测。
    #[tokio::test]
    async fn unknown_capability_backs_off_for_six_hours() {
        let (base_url, recorded) = mock_server(vec![(
            "/v1/chat/completions",
            400,
            r#"{"error":{"message":"Unrecognized request argument supplied: reasoning"}}"#,
        )]);

        let mut cached = ReasoningCapability::unknown(ReasoningKey::new(&base_url, "gpt-x"));
        assert_eq!(cached.ttl_seconds, TTL_UNKNOWN_SECONDS);

        // 窗口内：零请求。
        let fresh = discover_reasoning_capability(
            &test_client(), ProtocolKind::Openai, &base_url, "gpt-x", "key",
            MetadataSource::Attempted, Some(&cached),
        )
        .await;
        assert!(recorded.lock().unwrap().is_empty(), "Unknown 在 6 小时窗口内仍发起了请求");
        assert!(!fresh.changed);
        assert_eq!(fresh.capability.support, ReasoningSupport::Unknown);

        // 窗口过后（7 小时前发现）：重新探测，并拿到真实结论。
        cached.discovered_at = (chrono::Utc::now() - chrono::Duration::hours(7)).to_rfc3339();
        let expired = discover_reasoning_capability(
            &test_client(), ProtocolKind::Openai, &base_url, "gpt-x", "key",
            MetadataSource::Attempted, Some(&cached),
        )
        .await;
        assert_eq!(recorded.lock().unwrap().len(), 1, "窗口过期后未重新探测");
        assert_eq!(expired.capability.support, ReasoningSupport::Unsupported);
        assert_eq!(expired.capability.ttl_seconds, TTL_UNSUPPORTED_SECONDS);
        assert!(expired.changed);
    }

    /// 要求 6：连续保存 Provider 不重复 validation probe。
    /// 模拟真实链路——第一次的产出作为第二次的缓存输入。
    #[tokio::test]
    async fn consecutive_saves_do_not_repeat_validation_probe() {
        let (base_url, recorded) = mock_server(vec![("/v1/chat/completions", 200, r#"{"id":"chatcmpl-1"}"#)]);
        let client = test_client();
        let mut cached: Option<ReasoningCapability> = None;

        for round in 0..3 {
            let outcome = discover_reasoning_capability(
                &client, ProtocolKind::Openai, &base_url, "gpt-x", "key",
                MetadataSource::Attempted, cached.as_ref(),
            )
            .await;
            cached = Some(outcome.capability);
            assert_eq!(
                recorded.lock().unwrap().len(),
                1,
                "第 {} 轮保存重复发起了探测",
                round + 1
            );
        }
    }

    /// 要求 1/2：探测请求带自识别头，且输出上限被压到 1 token。
    #[tokio::test]
    async fn validation_probe_is_labelled_and_output_capped() {
        // (协议, 模型, 探测端点片段, 上限所在的 JSON 指针)
        let cases: Vec<(ProtocolKind, &str, &str, Vec<&str>)> = vec![
            (ProtocolKind::Openai, "gpt-x", "/v1/chat/completions", vec!["/max_tokens", "/max_completion_tokens"]),
            (ProtocolKind::Anthropic, "claude-x", "/v1/messages", vec!["/max_tokens"]),
            (
                ProtocolKind::Gemini,
                "gemini-x",
                "/v1beta/models/gemini-x:generateContent",
                vec!["/generationConfig/maxOutputTokens"],
            ),
        ];

        for (protocol, model, endpoint, pointers) in cases {
            let (base_url, recorded) = mock_server(vec![(endpoint, 400, r#"{"error":{"message":"nothing useful"}}"#)]);
            discover_reasoning_capability(
                &test_client(), protocol, &base_url, model, "key",
                MetadataSource::Attempted, None,
            )
            .await;

            let requests = recorded.lock().unwrap().clone();
            let probe = requests
                .iter()
                .find(|request| request.contains(endpoint))
                .unwrap_or_else(|| panic!("{protocol:?} 未发出 validation probe，实际请求：{requests:?}"));

            assert!(
                probe.to_lowercase().contains(&format!("{PROBE_HEADER}: {CAPABILITY_VALIDATION_PROBE}")),
                "{protocol:?} 的探测请求缺少自识别头：{probe}"
            );

            let body: Value = probe
                .split_once("\r\n\r\n")
                .and_then(|(_, body)| serde_json::from_str(body).ok())
                .unwrap_or_else(|| panic!("{protocol:?} 的请求体无法解析：{probe}"));
            for pointer in pointers {
                assert_eq!(
                    body.pointer(pointer).and_then(Value::as_u64),
                    Some(1),
                    "{protocol:?} 的 {pointer} 不是 1：{body}"
                );
            }
        }
    }

    /// Gemini 的探测体原本完全没有输出上限，回归保护。
    #[tokio::test]
    async fn gemini_probe_never_ships_without_output_cap() {
        let (base_url, recorded) = mock_server(vec![(
            ":generateContent",
            400,
            r#"{"error":{"message":"nothing useful"}}"#,
        )]);
        discover_reasoning_capability(
            &test_client(), ProtocolKind::Gemini, &base_url, "models/gemini-x", "key",
            MetadataSource::Attempted, None,
        )
        .await;

        let requests = recorded.lock().unwrap().clone();
        let probe = requests.iter().find(|request| request.contains(":generateContent")).expect("未发出探测");
        assert!(probe.contains("maxOutputTokens"), "Gemini 探测缺少输出上限：{probe}");
    }

    /// TTL 过期触发重新探测（对照未过期短路用例）。
    #[tokio::test]
    async fn stale_cache_triggers_rediscovery() {
        let (base_url, recorded) = mock_server(vec![(
            "/v1/models/gpt-x",
            200,
            r#"{"id":"gpt-x","capabilities":{"reasoning":{"effort":["low","medium","high","xhigh"]}}}"#,
        )]);
        let cached = stale(supported_cache(&base_url, "gpt-x"));
        assert!(cached.should_rediscover());

        let outcome = discover_reasoning_capability(
            &test_client(),
            ProtocolKind::Openai,
            &base_url,
            "gpt-x",
            "key",
            MetadataSource::Absent,
            Some(&cached),
        )
        .await;

        assert_eq!(recorded.lock().unwrap().len(), 1);
        assert_eq!(outcome.capability.ttl_seconds, TTL_SUPPORTED_SECONDS);
        assert!(!outcome.capability.is_stale(), "重新探测后仍是过期状态");
        assert!(outcome.changed);
    }

    /// 上层据此判断能否复用已取到的模型详情响应体，避免重复请求。
    #[test]
    fn metadata_endpoint_matching_is_protocol_driven() {
        assert!(metadata_endpoint_matches(
            "https://api.example.com/v1",
            ProtocolKind::Openai,
            "gpt-x",
            "https://api.example.com/v1/models/gpt-x"
        ));
        // Gemini 的元数据端点在 /v1beta，与通用 /v1/models/{id} 不同源，不可复用。
        assert!(!metadata_endpoint_matches(
            "https://api.example.com/v1",
            ProtocolKind::Gemini,
            "models/gemini-x",
            "https://api.example.com/v1/models/models%2Fgemini-x"
        ));
    }

    #[test]
    fn azure_stays_out_of_discovery() {
        assert!(supports_discovery(ProtocolKind::Openai));
        assert!(supports_discovery(ProtocolKind::Anthropic));
        assert!(supports_discovery(ProtocolKind::Gemini));
        assert!(!supports_discovery(ProtocolKind::AzureOpenai));
    }
}
