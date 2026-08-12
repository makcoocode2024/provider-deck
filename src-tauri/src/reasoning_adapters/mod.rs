use serde_json::Value;

use crate::{
    error::AppResult,
    model::ProtocolKind,
    reasoning_capability::{ReasoningCapability, ReasoningKey},
};

mod openai;
mod anthropic;
mod gemini;

pub use openai::OpenAIAdapter;
pub use anthropic::AnthropicAdapter;
pub use gemini::GeminiAdapter;

/// Tier 0 metadata 的数据来源端点，形如 /v1/models/{id}。
/// 与 Tier 1 分开是因为这个响应体在现有 context_window 流程里已经取到了，可零成本复用。
#[derive(Debug, Clone)]
pub struct MetadataTarget {
    pub endpoint: String,
    pub auth: AuthScheme,
}

/// 元数据线索：从 Provider 返回的模型列表提取的能力暗示。
#[derive(Debug, Clone, Default)]
pub struct MetadataHints {
    /// 模型元数据 JSON 中与推理相关的字段名/值片段，用于 Tier 0 快速判断。
    pub reasoning_fields: Vec<String>,
    /// 探测到的 effort 枚举成员候选（OpenAI style）。
    pub effort_values: Vec<String>,
    /// 探测到的预算区间暗示（Anthropic/Gemini style）。
    pub budget_range: Option<(u64, u64)>,
    /// 动态预算哨兵值（Gemini -1）。
    pub dynamic_sentinel: Option<i64>,
    /// 该模型是否无法关闭推理（某些 Gemini 2.5 Pro）。
    pub cannot_disable: bool,
}

/// 该协议的鉴权方式。让探测请求自描述，编排层无需知道协议是什么。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthScheme {
    /// Authorization: Bearer {key}
    Bearer,
    /// x-api-key: {key} + anthropic-version
    AnthropicKey,
    /// x-goog-api-key: {key}
    GoogleKey,
}

/// 能力查询端点：Tier 1 introspection 目标，通常免费且无副作用。
#[derive(Debug, Clone)]
pub struct IntrospectionTarget {
    /// 相对于 base_url 的目标路径，形如 /v1beta/models/{id}。
    pub endpoint: String,
    /// 请求 method（GET / POST）。
    pub method: &'static str,
    /// 该端点的鉴权方式。
    pub auth: AuthScheme,
    /// 期望从响应中提取的 JSON pointer 路径。
    pub extract_path: Vec<String>,
}

/// Tier 2 探测请求的自识别头名称。沿用工程既有惯例（见 protocol.rs 的连通性与
/// Codex 兼容性探测），服务商日志据此可把能力验证与真实用户对话区分开。
pub const PROBE_HEADER: &str = "x-provider-deck-probe";

/// Tier 2 的自识别值。
pub const CAPABILITY_VALIDATION_PROBE: &str = "capability-validation";

/// Tier 2 允许的最大输出 token。
///
/// 这个值**只能**由编排层使用，Adapter 不得自行决定输出上限——成本约束必须有单一
/// 强制点，否则新增一个 Adapter 就多一处可能忘记设上限的地方（Gemini 的 probe 原本
/// 就完全没有输出上限）。
pub const VALIDATION_MAX_OUTPUT_TOKENS: u64 = 1;

/// 输出上限补丁：Adapter 声明"本协议把输出上限写在哪个字段"，编排层在发送前统一写值。
///
/// 只有指针、没有取值，是刻意的：取值来自 [`VALIDATION_MAX_OUTPUT_TOKENS`]，
/// 由编排层填。同一个协议可以声明多个候选（例如 OpenAI 的 `max_tokens` 与
/// `max_completion_tokens`），全部写入，命中哪个由服务端决定。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputLimitPatch {
    /// JSON 指针，形如 `/max_tokens`、`/generationConfig/maxOutputTokens`。
    /// 缺失的中间对象由编排层按需创建。
    pub pointer: String,
}

impl OutputLimitPatch {
    pub fn at(pointer: impl Into<String>) -> Self {
        Self { pointer: pointer.into() }
    }
}

/// 验证探测请求：Tier 2 capability validation probe，通过发送超出范围的值触发参数校验错误。
///
/// 这个请求会打到真实推理端点，属于产品定位允许的小成本能力验证。三条约束由编排层保证：
/// 带 [`PROBE_HEADER`] 自识别、输出上限压到 [`VALIDATION_MAX_OUTPUT_TOKENS`]、结果落
/// evidence 与缓存 TTL。
#[derive(Debug, Clone)]
pub struct ValidationProbe {
    /// 相对于 base_url 的目标端点（通常是 chat/completions 或 messages）。
    pub endpoint: String,
    /// 完整请求体 JSON，已包含故意越界的推理参数。
    pub body: Value,
    /// 该端点的鉴权方式。
    pub auth: AuthScheme,
    /// 该协议的输出上限字段候选。编排层发送前逐个写入，一个都写不进去时放弃本次探测。
    pub output_limits: Vec<OutputLimitPatch>,
}

/// 错误解读结果：从 400/422 响应推断能力状态。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ErrorInterpretation {
    /// 错误消息明确枚举了支持的取值或范围，可提取为能力描述。
    Supported { detail: String },
    /// 明确拒绝该参数（unrecognized field / not supported）。
    Unsupported { detail: String },
    /// 无法从错误中推断，需要更高 tier 的证据。
    Unknown,
}

/// Provider 能力发现适配器。每个协议一个实现，屏蔽协议差异。
pub trait ReasoningAdapter: Send + Sync {
    /// 从 /v1/models 返回的单个模型元数据中提取推理能力线索（Tier 0）。
    fn metadata_hints(&self, model_json: &Value) -> MetadataHints;

    /// Tier 0 元数据的获取端点。编排层若手里已有该响应体（现有 context_window 流程会取），
    /// 可跳过这次请求；否则按此端点自行获取。
    fn metadata_target(&self, model_id: &str) -> Option<MetadataTarget>;

    /// 返回该协议的能力查询端点列表（Tier 1）。
    /// 默认返回空，表示该协议没有免费的 introspection 接口。
    fn introspection_targets(&self, _model_id: &str) -> Vec<IntrospectionTarget> {
        Vec::new()
    }

    /// 构造一个验证探测请求（Tier 2）：发送故意越界的参数值，触发校验错误。
    ///
    /// 实现者不要在 body 里操心输出上限——`output_limits` 声明字段位置即可，
    /// 编排层会在发送前统一压到 [`VALIDATION_MAX_OUTPUT_TOKENS`]。
    fn validation_probe(&self, model_id: &str) -> Option<ValidationProbe>;

    /// 解读 400/422 错误响应，判断是"明确支持（并枚举取值）"还是"明确不支持"。
    fn interpret_error(&self, status: u16, body: &Value) -> ErrorInterpretation;

    /// 将抽象的 ReasoningCapability 映射为该协议的请求参数（供 Step 5 写出端和本地代理使用）。
    /// 入参：完整的能力描述 + 用户选择的档位（或自动推荐的档位）。
    /// 返回：该协议的 JSON 字段片段，由调用方合并到请求体中。
    fn apply_reasoning_config(&self, capability: &ReasoningCapability, tier: crate::reasoning_capability::ReasoningTier) -> Option<Value>;

    /// 从响应中提取推理 token 用量（Tier 4 被动观测）。
    /// 返回 (input_reasoning_tokens, output_reasoning_tokens)。
    fn observe_usage(&self, _response: &Value) -> (Option<u64>, Option<u64>) {
        (None, None)
    }

    /// 检查响应是否包含推理相关字段（用于 Runtime Verification）。
    ///
    /// 返回 true 表示响应中包含该协议的推理产物标识（如 reasoning_tokens、thinking block、thinkingTokenCount）。
    /// 此方法不负责修改 capability、confidence 或写入 verification 记录，仅判断响应特征。
    fn has_reasoning_in_response(&self, response: &Value) -> bool;
}

/// 根据协议类型获取对应的 Adapter。
pub fn adapter_for(protocol: ProtocolKind) -> Box<dyn ReasoningAdapter> {
    match protocol {
        ProtocolKind::Openai | ProtocolKind::AzureOpenai => Box::new(OpenAIAdapter),
        ProtocolKind::Anthropic => Box::new(AnthropicAdapter),
        ProtocolKind::Gemini => Box::new(GeminiAdapter),
        ProtocolKind::Custom => Box::new(OpenAIAdapter), // Custom 默认走 OpenAI 兼容
    }
}

/// 把 Tier 2 探测体的输出上限统一压到 [`VALIDATION_MAX_OUTPUT_TOKENS`]。
///
/// 返回实际写入成功的指针数量。0 表示该协议没有任何可用的上限字段——编排层据此
/// 放弃探测，宁可结论 Unknown 也不发一个没有输出上限的请求。
///
/// 无条件覆写而非"仅在缺失时填充"：Adapter 里写的任何值（如 OpenAI 原本的 10）
/// 都必须被压到 1，成本上限是流程属性，不是 Adapter 的自觉。
pub fn enforce_output_limits(body: &mut Value, limits: &[OutputLimitPatch]) -> usize {
    let mut applied = 0;
    for patch in limits {
        if write_pointer(body, &patch.pointer, Value::from(VALIDATION_MAX_OUTPUT_TOKENS)) {
            applied += 1;
        }
    }
    applied
}

/// 按 JSON 指针写值，缺失的中间对象按需创建。
/// serde_json 只提供 `pointer_mut`（要求路径已存在），这里补上创建语义。
fn write_pointer(root: &mut Value, pointer: &str, value: Value) -> bool {
    let segments: Vec<&str> = pointer.trim_start_matches('/').split('/').filter(|s| !s.is_empty()).collect();
    let Some((last, parents)) = segments.split_last() else { return false };

    let mut cursor = root;
    for segment in parents {
        if !cursor.is_object() { return false; }
        cursor = cursor
            .as_object_mut()
            .expect("已确认是 object")
            .entry((*segment).to_owned())
            .or_insert_with(|| Value::Object(serde_json::Map::new()));
    }

    let Some(object) = cursor.as_object_mut() else { return false };
    object.insert((*last).to_owned(), value);
    true
}

/// 从元数据线索合成 ReasoningCapability（Tier 0 证据）。
/// 这是 Adapter 的辅助函数，不属于 trait 方法——因为合成逻辑是协议无关的。
pub fn capability_from_metadata(
    key: ReasoningKey,
    hints: MetadataHints,
) -> Option<ReasoningCapability> {
    use crate::reasoning_capability::{EvidenceSource, ReasoningCapability, ReasoningConfidence, ReasoningEvidence};

    if hints.reasoning_fields.is_empty() && hints.effort_values.is_empty() && hints.budget_range.is_none() {
        return None;
    }

    let mut capability = if !hints.effort_values.is_empty() {
        ReasoningCapability::from_effort_enum(key, &hints.effort_values, ReasoningConfidence::Declared)
    } else if let Some((min, max)) = hints.budget_range {
        ReasoningCapability::from_token_budget(key, min, max, true, hints.dynamic_sentinel, ReasoningConfidence::Declared)
    } else {
        ReasoningCapability::from_boolean_toggle(key, !hints.cannot_disable, ReasoningConfidence::Declared)
    };

    for field in hints.reasoning_fields {
        capability.push_evidence(ReasoningEvidence::new(
            EvidenceSource::ModelListMetadata,
            None,
            format!("模型元数据包含推理字段：{field}"),
        ));
    }

    if hints.cannot_disable {
        capability.constraints.cannot_disable = true;
        capability.constraints.notes.push("该模型无法关闭推理能力".into());
    }

    Some(capability)
}

/// 从 validation probe 错误解读合成 ReasoningCapability（Tier 2 证据）。
pub fn capability_from_validation(
    key: ReasoningKey,
    interpretation: ErrorInterpretation,
    endpoint: &str,
) -> AppResult<ReasoningCapability> {
    use crate::reasoning_capability::{EvidenceSource, ReasoningCapability, ReasoningConfidence, ReasoningEvidence};
    use crate::error::AppError;

    match interpretation {
        ErrorInterpretation::Supported { detail } => {
            let mut capability = ReasoningCapability::unknown(key);
            capability.confidence = ReasoningConfidence::Validated;
            capability.push_evidence(ReasoningEvidence::new(
                EvidenceSource::CapabilityValidation,
                Some(endpoint.to_owned()),
                detail,
            ));
            Ok(capability)
        }
        ErrorInterpretation::Unsupported { detail } => {
            let mut capability = ReasoningCapability::unsupported(key, ReasoningConfidence::Validated);
            capability.push_evidence(ReasoningEvidence::new(
                EvidenceSource::CapabilityValidation,
                Some(endpoint.to_owned()),
                detail,
            ));
            Ok(capability)
        }
        ErrorInterpretation::Unknown => Err(AppError::InvalidInput("无法从错误响应推断能力状态".into())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reasoning_capability::{ReasoningControl, ReasoningSupport, ReasoningTier};
    use serde_json::json;

    fn key() -> ReasoningKey {
        ReasoningKey::new("https://api.example.com/v1", "some-model")
    }

    /// 协议到 Adapter 的唯一分派点。核心业务代码只调用它，不做 provider 分支。
    #[test]
    fn dispatch_covers_every_protocol() {
        for protocol in [
            ProtocolKind::Openai,
            ProtocolKind::Anthropic,
            ProtocolKind::Gemini,
            ProtocolKind::AzureOpenai,
            ProtocolKind::Custom,
        ] {
            let adapter = adapter_for(protocol);
            assert!(adapter.validation_probe("m").is_some());
        }
    }

    #[test]
    fn empty_hints_produce_no_capability() {
        assert!(capability_from_metadata(key(), MetadataHints::default()).is_none());
    }

    #[test]
    fn effort_hints_take_priority_over_budget() {
        let hints = MetadataHints {
            reasoning_fields: vec!["capabilities.reasoning".into()],
            effort_values: vec!["low".into(), "high".into()],
            budget_range: Some((1024, 8192)),
            ..MetadataHints::default()
        };
        let capability = capability_from_metadata(key(), hints).expect("capability missing");
        assert!(matches!(capability.control, ReasoningControl::EffortEnum { .. }));
        assert_eq!(capability.confidence, ReasoningConfidenceAlias::Declared);
        assert_eq!(capability.evidence.len(), 1);
        assert_eq!(capability.evidence[0].source, EvidenceSourceAlias::ModelListMetadata);
    }

    #[test]
    fn cannot_disable_hint_lands_in_constraints() {
        let hints = MetadataHints {
            reasoning_fields: vec!["thinkingConfig".into()],
            cannot_disable: true,
            ..MetadataHints::default()
        };
        let capability = capability_from_metadata(key(), hints).expect("capability missing");
        assert!(capability.constraints.cannot_disable);
        assert!(capability.tier(ReasoningTier::Off).is_none());
    }

    #[test]
    fn validation_supported_records_evidence_without_guessing_tiers() {
        let capability = capability_from_validation(
            key(),
            ErrorInterpretation::Supported { detail: "枚举了取值".into() },
            "/v1/chat/completions",
        )
        .expect("capability missing");
        // 只提升置信度并留证据，不凭空造档位——档位要等真正拿到取值集合。
        assert_eq!(capability.confidence, ReasoningConfidenceAlias::Validated);
        assert!(capability.tiers.is_empty());
        assert_eq!(capability.evidence[0].endpoint.as_deref(), Some("/v1/chat/completions"));
        assert_eq!(capability.evidence[0].source, EvidenceSourceAlias::CapabilityValidation);
    }

    /// 要求 1：三个协议的探测体都必须能被压到 1 token 输出。
    /// 断言的是"编排层写得进去"，而不是"Adapter 自己写对了"。
    #[test]
    fn every_protocol_declares_a_writable_output_limit() {
        for protocol in [ProtocolKind::Openai, ProtocolKind::Anthropic, ProtocolKind::Gemini] {
            let mut probe = adapter_for(protocol).validation_probe("m").expect("probe missing");
            assert!(!probe.output_limits.is_empty(), "{protocol:?} 未声明输出上限字段");
            let applied = enforce_output_limits(&mut probe.body, &probe.output_limits);
            assert_eq!(applied, probe.output_limits.len(), "{protocol:?} 有上限指针写入失败");
            for patch in &probe.output_limits {
                assert_eq!(
                    probe.body.pointer(&patch.pointer).and_then(Value::as_u64),
                    Some(VALIDATION_MAX_OUTPUT_TOKENS),
                    "{protocol:?} 的 {} 未被压到 1",
                    patch.pointer
                );
            }
        }
    }

    /// Tier 1 是"免费无副作用"这一层，成本闸门（enforce_output_limits）只作用于 Tier 2。
    /// 一旦某个 Adapter 把 introspection 声明成 POST，它就能绕过闸门打到生成端点，
    /// 且 IntrospectionTarget 结构里没有任何地方能声明输出上限。
    #[test]
    fn introspection_targets_stay_side_effect_free() {
        for protocol in [
            ProtocolKind::Openai,
            ProtocolKind::Anthropic,
            ProtocolKind::Gemini,
            ProtocolKind::AzureOpenai,
            ProtocolKind::Custom,
        ] {
            for target in adapter_for(protocol).introspection_targets("m") {
                assert_eq!(
                    target.method, "GET",
                    "{protocol:?} 的 introspection 端点 {} 不是 GET：Tier 1 没有输出上限闸门",
                    target.endpoint
                );
            }
        }
    }

    /// Tier 2 的探测体不得声明 stream：流式响应下输出上限的实际效果依赖服务端实现，
    /// 且响应无法用 `response.json()` 一次读完，编排层的解析会退化成 Opaque。
    #[test]
    fn validation_probes_never_stream() {
        for protocol in [ProtocolKind::Openai, ProtocolKind::Anthropic, ProtocolKind::Gemini] {
            let probe = adapter_for(protocol).validation_probe("m").expect("probe missing");
            assert_ne!(
                probe.body.get("stream").and_then(Value::as_bool),
                Some(true),
                "{protocol:?} 的探测体开启了 stream"
            );
        }
    }

    /// Adapter 里写死的上限必须被无条件覆写，不能"仅在缺失时填充"。
    #[test]
    fn existing_limit_values_are_overwritten() {
        let mut body = serde_json::json!({ "max_tokens": 4096 });
        assert_eq!(enforce_output_limits(&mut body, &[OutputLimitPatch::at("/max_tokens")]), 1);
        assert_eq!(body["max_tokens"], 1);
    }

    /// 嵌套指针的中间对象缺失时按需创建（Gemini 的 generationConfig 情形）。
    #[test]
    fn nested_pointer_creates_missing_objects() {
        let mut body = serde_json::json!({ "contents": [] });
        assert_eq!(
            enforce_output_limits(&mut body, &[OutputLimitPatch::at("/generationConfig/maxOutputTokens")]),
            1
        );
        assert_eq!(body["generationConfig"]["maxOutputTokens"], 1);
    }

    /// 指针指向非对象节点时写入失败，返回 0——编排层据此放弃探测。
    #[test]
    fn pointer_into_non_object_fails_instead_of_panicking() {
        let mut body = serde_json::json!({ "generationConfig": 7 });
        assert_eq!(
            enforce_output_limits(&mut body, &[OutputLimitPatch::at("/generationConfig/maxOutputTokens")]),
            0
        );
    }

    #[test]
    fn validation_unsupported_marks_support_false() {
        let capability = capability_from_validation(
            key(),
            ErrorInterpretation::Unsupported { detail: "拒绝该参数".into() },
            "/v1/messages",
        )
        .expect("capability missing");
        assert_eq!(capability.support, ReasoningSupport::Unsupported);
        assert_eq!(capability.ttl_seconds, crate::reasoning_capability::TTL_UNSUPPORTED_SECONDS);
    }

    #[test]
    fn validation_unknown_is_an_error_not_a_guess() {
        assert!(capability_from_validation(key(), ErrorInterpretation::Unknown, "/v1/x").is_err());
    }

    /// 三个 Adapter 都不能把 unknown 能力映射出任何参数。
    #[test]
    fn no_adapter_emits_params_for_unknown_capability() {
        let capability = ReasoningCapability::unknown(key());
        for protocol in [ProtocolKind::Openai, ProtocolKind::Anthropic, ProtocolKind::Gemini] {
            let adapter = adapter_for(protocol);
            for tier in [ReasoningTier::Off, ReasoningTier::Standard, ReasoningTier::Max] {
                assert!(adapter.apply_reasoning_config(&capability, tier).is_none());
            }
        }
    }

    /// 未知错误格式一律返回 Unknown，绝不猜测。
    #[test]
    fn unrelated_errors_stay_unknown() {
        let body = json!({ "error": { "message": "rate limit exceeded" } });
        for protocol in [ProtocolKind::Openai, ProtocolKind::Anthropic, ProtocolKind::Gemini] {
            assert_eq!(adapter_for(protocol).interpret_error(400, &body), ErrorInterpretation::Unknown);
        }
    }

    use crate::reasoning_capability::EvidenceSource as EvidenceSourceAlias;
    use crate::reasoning_capability::ReasoningConfidence as ReasoningConfidenceAlias;
}
