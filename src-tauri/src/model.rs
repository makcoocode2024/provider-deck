use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::reasoning_capability::{ReasoningCapability, ReasoningTier};
use crate::reasoning_selection::ReasoningSelection;
use crate::reasoning_verification::RuntimeVerification;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ProtocolKind {
    Openai,
    Anthropic,
    Gemini,
    AzureOpenai,
    Custom,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ClaudeModelProfile {
    Sonnet,
    Opus,
    Haiku,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum CodexCompatibility {
    Full,
    FunctionToolsOnly,
    ChatProxy,
    ResponsesUnsupported,
    Unknown,
    NotApplicable,
}

impl Default for CodexCompatibility {
    fn default() -> Self { Self::Unknown }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeModelMappings {
    pub sonnet: Option<String>,
    pub opus: Option<String>,
    pub haiku: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelInfo {
    pub id: String,
    pub display_name: String,
    pub provider: Option<String>,
    pub protocol: ProtocolKind,
    pub source: String,
    pub capabilities: Vec<String>,
    pub context_window: Option<u64>,
    #[serde(default)]
    pub parameter_count_billions: Option<f64>,
    #[serde(default)]
    pub reasoning: Option<ReasoningCapability>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ReasoningLevel {
    Low,
    Medium,
    High,
}

impl Default for ReasoningLevel {
    fn default() -> Self { Self::High }
}

impl ReasoningLevel {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
        }
    }
}

/// 用户为某个具体模型指定的兜底档位。
///
/// 三点定性：
///
/// 1. **这是用户设定，不是探测事实。** 它绝不写进 `ModelInfo.reasoning`，也绝不影响
///    confidence。能力探明之后它自动失效——不需要迁移代码，`config::codex_reasoning`
///    只在能力缺失或 Unknown 时才看它。
/// 2. **按精确 `model_id` 匹配。** 这条不因为新增了名称规则而放松：本结构仍然只做全等
///    比较。模糊匹配是 [`ReasoningNameRule`] 的职责，两者刻意分成两张表，因为它们的
///    风险等级不同——全等匹配不可能误伤别的模型，模式匹配可能。
/// 3. **档位用 `tier_id` 而不是具体参数。** 内置档位用固定 id（off/light/standard/
///    deep/max），自定义档位用其 uuid。存 id 而不存参数：用户改了自定义档位的参数，
///    引用它的规则应当自动跟到新值。
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ReasoningFallback {
    pub model_id: String,
    pub tier_id: String,
}

/// 旧字段 `level: "low"|"medium"|"high"` 到 `tier_id` 的兼容读取。
///
/// 手写 `Deserialize` 而不是留一个 `Option<ReasoningLevel>` 影子字段：影子字段会一直
/// 留在序列化输出里，下一个读到它的人无从判断哪个才是权威值。这里在**入口**一次性
/// 归一，落盘就只有 `tierId`。
impl<'de> Deserialize<'de> for ReasoningFallback {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Raw {
            #[serde(default)]
            model_id: String,
            #[serde(default)]
            tier_id: Option<String>,
            /// 只读不写。旧 state.json 里这一档是 ReasoningLevel。
            #[serde(default)]
            level: Option<ReasoningLevel>,
        }
        let raw = Raw::deserialize(deserializer)?;
        // tierId 优先：两者都在时说明是新版写出的文件，level 只是残留。
        let tier_id = raw.tier_id
            .or_else(|| raw.level.map(|level| ReasoningTier::from_legacy(level).as_str().to_owned()))
            .unwrap_or_default();
        Ok(Self { model_id: raw.model_id, tier_id })
    }
}

/// 模型名匹配方式。只有这两种，刻意不提供正则：
/// 正则的误伤范围无法在界面上预估，而这张表的每一条都会影响配置写出。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum NameMatchType {
    Prefix,
    Contains,
}

impl NameMatchType {
    pub fn label(self) -> &'static str {
        match self {
            Self::Prefix => "前缀匹配",
            Self::Contains => "包含匹配",
        }
    }
}

/// 按模型名匹配的兜底规则。
///
/// **这不是"根据模型名推断能力"。** 区别在于证据来源：推断是程序自己认为
/// `xxx-thinking` 大概支持推理；本规则是用户明确写下"我这批模型名以 x 开头的，配置里
/// 按这个档位写"。程序不预置任何规则，初始为空表，一条也不生成。
///
/// 命中结果只影响配置文件写出，绝不写进 `ModelInfo.reasoning`，也绝不影响
/// confidence——它连 `ReasoningSupport` 都不会去改。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ReasoningNameRule {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub pattern: String,
    #[serde(default = "default_match_type")]
    pub match_type: NameMatchType,
    /// 引用的档位 id：内置固定 id 或自定义档位的 uuid。
    /// 被引用的档位删掉之后这里会指向不存在的 id，此时整条规则跳过，不报错。
    #[serde(default)]
    pub tier_id: String,
}

fn default_match_type() -> NameMatchType { NameMatchType::Prefix }

/// 用户自定义的推理档位。
///
/// 存在的理由：内置 5 档只能映射成 OpenAI 的 effort 词表，因为「深度推理对应多少
/// budget_tokens」在未探明的模型上没有任何证据可依。用户知道自己那个网关认什么，
/// 就让用户直接写协议参数——这是把"取值必须有出处"的出处换成用户，而不是取消它。
///
/// 三个协议参数各自独立且都可为空：只填 openai 的档位在 Anthropic 端点上不生效，
/// 走降级链而不是硬套一个编造的值。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CustomReasoningTier {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub openai_params: Option<serde_json::Value>,
    #[serde(default)]
    pub anthropic_params: Option<serde_json::Value>,
    #[serde(default)]
    pub gemini_params: Option<serde_json::Value>,
}

impl CustomReasoningTier {
    /// 该档位是否为任何协议配了参数。全空的档位保存不了，也不该出现在下拉里。
    pub fn has_any_params(&self) -> bool {
        self.openai_params.is_some() || self.anthropic_params.is_some() || self.gemini_params.is_some()
    }

    /// 这个档位为哪些协议配了参数。界面用它说明"该档位在当前端点是否有可写的表达"。
    pub fn supported_protocols(&self) -> Vec<ProtocolKind> {
        let mut kinds = Vec::new();
        // OpenAI 系三个协议共用同一份参数（见 resolve_tier_config 的分支），所以一并列出，
        // 而不是只报 Openai——只报一个会让 Azure 端点上的用户以为档位不生效。
        if self.openai_params.is_some() {
            kinds.extend([ProtocolKind::Openai, ProtocolKind::AzureOpenai, ProtocolKind::Custom]);
        }
        if self.anthropic_params.is_some() { kinds.push(ProtocolKind::Anthropic); }
        if self.gemini_params.is_some() { kinds.push(ProtocolKind::Gemini); }
        kinds
    }
}

/// 模型原生推理参数的**类别**。
///
/// 存类别而不存字段名（`thinkingBudget` / `budget_tokens` / `reasoning.effort`）：字段名是
/// 协议知识，已经由 `reasoning_adapters` 各自持有，在这里再存一份等于把适配器知识复制到
/// serde 契约层，将来新增协议要改两处。
///
/// 取值只能由 `ReasoningControl` 推出，见 [`crate::reasoning_selection::native_param_kind`]。
/// 默认 `Unknown`：探不到就是探不到，不许落到任何具体类别。
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum NativeParamKind {
    #[default]
    Unknown,
    EffortEnum,
    TokenBudget,
    BooleanToggle,
}

/// 一条命中当前模型的自定义档位，带上"是被哪条规则怎么命中的"。
///
/// 命中说明必须一起返回：用户的规则表可能很长，只告诉他"这个档位适配"，他无法在表里
/// 找出是哪一条生效，也就无从修改。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MatchedCustomTier {
    #[serde(default)]
    pub tier_id: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub rule_pattern: String,
    #[serde(default = "default_match_type")]
    pub rule_match_type: NameMatchType,
    /// 该档位配了参数的协议清单。空表示这个档位在任何协议下都写不出参数。
    #[serde(default)]
    pub supported_protocols: Vec<ProtocolKind>,
}

/// 某个模型在某个端点下的推理档位可选面汇总，供界面一次取齐。
///
/// **这是只读投影，不是新的一级事实。** 每个字段都从已有数据推出：
/// `native_param_kind` / `builtin_tiers_compatible` 来自 `ModelInfo.reasoning`，
/// `matched_custom_tiers` 来自用户自己写的规则表。本结构的产生过程不发出站请求、
/// 不写 `ModelInfo.reasoning`、不动 confidence。要真的重探走 `reprobe_model_reasoning`。
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ModelReasoningMeta {
    /// 该模型在当前端点上能用推理的协议。能力未探明或探到不支持时为空。
    #[serde(default)]
    pub supported_protocols: Vec<ProtocolKind>,
    #[serde(default)]
    pub native_param_kind: NativeParamKind,
    /// 按用户规则表顺序排列的全部适配自定义档位。无匹配是空表，不填充任何默认档。
    #[serde(default)]
    pub matched_custom_tiers: Vec<MatchedCustomTier>,
    /// 内置五档能否用在这个模型上。
    ///
    /// 三态而非 bool：`None` 是"无法确认"，写成 `false` 会把未探明伪装成不兼容。
    #[serde(default)]
    pub builtin_tiers_compatible: Option<bool>,
}

/// 探测投影的时效缓存条目。
///
/// 只缓存能力投影那几项，**不缓存 `matched_custom_tiers`**：匹配结果随用户改规则立刻
/// 失效，缓存它必然读到脏数据，而重算它只是一次内存遍历。
///
/// 同样不缓存 `supported_protocols`：它由端点协议加上"是否探明支持"推出，两者调用时都在手上。
///
/// 安全约束：本条目 MUST NOT 出现密钥、请求体、响应体。有单测
/// `detection_cache_carries_no_secrets` 钉住。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ReasoningDetectionCacheEntry {
    #[serde(default)]
    pub base_url: String,
    #[serde(default)]
    pub model_id: String,
    #[serde(default)]
    pub detected_at: String,
    #[serde(default)]
    pub ttl_seconds: u64,
    #[serde(default)]
    pub native_param_kind: NativeParamKind,
    #[serde(default)]
    pub builtin_tiers_compatible: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Provider {
    pub id: String,
    pub name: String,
    pub base_url: String,
    pub protocol: ProtocolKind,
    pub enabled: bool,
    pub is_current: bool,
    pub default_model: Option<String>,
    #[serde(default)]
    pub claude_model_profile: Option<ClaudeModelProfile>,
    #[serde(default)]
    pub claude_extended_context: bool,
    #[serde(default)]
    pub claude_model_mappings: ClaudeModelMappings,
    #[serde(default)]
    pub codex_compatibility: CodexCompatibility,
    #[serde(default)]
    pub codex_probe_model: Option<String>,
    #[serde(default)]
    pub codex_probe_detail: Option<String>,
    /// 用户对本 Provider 各模型的推理档位选择。
    ///
    /// 挂在 Provider 而不是 ModelInfo：models 在 save/reprobe/refresh 三处都是整体替换，
    /// 把用户意图放进去每次刷新都要靠迁移逻辑救一次。挂这里只需按 model_id 剪枝。
    /// 同时守住"ModelInfo 装发现到的事实，Provider 装用户的意图"这条边界。
    #[serde(default)]
    pub reasoning_selections: Vec<ReasoningSelection>,
    /// 用户主动发起的运行时验证历史，key 为 model_id。
    ///
    /// 与 `ModelInfo.reasoning` 分开：那边装 discovery 探测到的能力事实，这里装用户行为记录。
    /// 挂 Provider 而不是 ModelInfo，是因为验证归属"此 endpoint 下的此模型"——
    /// base_url 变了旧记录一律作废，而 models 在 save/reprobe/refresh 三处整体替换，
    /// 挂进去每次刷新都要靠迁移逻辑救一次。
    #[serde(default)]
    pub reasoning_verifications: HashMap<String, Vec<RuntimeVerification>>,
    pub models: Vec<ModelInfo>,
    pub connection_state: String,
    pub confidence: Option<f64>,
    pub last_checked_at: Option<String>,
    pub applied_clients: Vec<String>,
    pub error_summary: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderDraft {
    pub id: Option<String>,
    pub name: String,
    pub base_url: String,
    pub api_key: String,
    pub protocol_hint: Option<ProtocolKind>,
    pub timeout_seconds: u64,
    pub azure_api_version: Option<String>,
    #[serde(default)]
    pub default_model: Option<String>,
    #[serde(default)]
    pub claude_model_profile: Option<ClaudeModelProfile>,
    #[serde(default)]
    pub claude_extended_context: bool,
    #[serde(default)]
    pub claude_model_mappings: ClaudeModelMappings,
    /// 前端提交的推理档位选择。可以只带当前正在编辑的模型，
    /// 合并时草稿优先、未提到的模型保留原值（见 reasoning_selection::merge_drafted）。
    #[serde(default)]
    pub reasoning_selections: Vec<ReasoningSelection>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProbeResult {
    pub normalized_base_url: String,
    pub protocol: ProtocolKind,
    pub confidence: f64,
    pub models: Vec<ModelInfo>,
    #[serde(default)]
    pub codex_compatibility: CodexCompatibility,
    #[serde(default)]
    pub codex_probe_model: Option<String>,
    #[serde(default)]
    pub codex_probe_detail: Option<String>,
    pub checked_endpoints: Vec<String>,
    pub user_message: String,
    pub technical_detail: Option<String>,
    /// 推理能力发现的说明（限流、端点不可达、需要真实请求才能确证等）。
    /// 与 codexProbeDetail 同级，供前端展示；能力本体挂在 ModelInfo.reasoning 上。
    #[serde(default)]
    pub reasoning_note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderTestCheck {
    pub id: String,
    pub label: String,
    pub status: String,
    pub detail: String,
    pub latency_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderTestReport {
    pub provider_id: String,
    pub model: Option<String>,
    pub total_latency_ms: u64,
    pub checks: Vec<ProviderTestCheck>,
    pub reply_preview: Option<String>,
}

/// 客户端适配级别。
///
/// 四档而不是"自动/手动"两档：`auto_config` 已经是那条布尔轴，这里区分的是
/// 适配器的核实程度——`verified` 的配置格式经过核对，`experimental` 的没有，
/// 两者的 `auto_config` 可以都为 true 但风险不同。合并成两档会让
/// codex-cli 与 gemini-cli 在界面上无法区分。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum SupportLevel {
    Verified,
    Experimental,
    /// 只检测和拉起，不写任何配置。
    #[default]
    Manual,
    Unsupported,
}

impl SupportLevel {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Verified => "verified",
            Self::Experimental => "experimental",
            Self::Manual => "manual",
            Self::Unsupported => "unsupported",
        }
    }
}

/// 客户端探测结果。
///
/// 只由 `clients::detect_all` 现场生成，从不进 `state.json`——所以字段增删不涉及
/// 存量数据迁移。`#[serde(default)]` 仍然全字段带上：这个结构会跨 IPC 到前端，
/// 旧前端缓存或回放的 JSON 少字段时应当降级，而不是整条反序列化失败。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientDescriptor {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub platforms: Vec<String>,
    #[serde(default)]
    pub protocols: Vec<ProtocolKind>,
    #[serde(default)]
    pub installed: bool,
    #[serde(default)]
    pub detected_path: Option<String>,
    #[serde(default)]
    pub config_path: Option<String>,
    /// 适配级别。缺省 `Manual`——少了这个字段时按"只读不写"降级，
    /// 反过来默认成 `Verified` 会让一个来路不明的客户端拿到写入资格。
    #[serde(default)]
    pub support: SupportLevel,
    #[serde(default)]
    pub auto_config: bool,
    #[serde(default)]
    pub requires_restart: bool,
    #[serde(default)]
    pub guidance: String,
    /// 启动器要拉起的可执行文件。
    ///
    /// 与 `detected_path` 分开：探测命中的可能是数据目录而不是可执行文件
    /// （MSIX 封装就是这样），拿它去 spawn 必然失败。None 表示无法可靠拉起。
    #[serde(default)]
    pub launch_target: Option<String>,
    /// 该客户端是否会从环境变量读取 API Key。
    ///
    /// 只有为 true 时启动器注入密钥才有意义。否则密钥进了子进程环境也没人读，
    /// 白担一份暴露风险——同一用户的任何进程都能看到子进程的环境块。
    #[serde(default)]
    pub env_injection: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigChange {
    pub client_id: String,
    pub client_name: String,
    pub target_path: Option<String>,
    pub support: String,
    pub can_write: bool,
    pub format: String,
    pub before_preview: String,
    pub after_preview: String,
    pub warnings: Vec<String>,
    pub expected_hash: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyResult {
    pub client_id: String,
    pub success: bool,
    pub backup_id: Option<String>,
    pub message: String,
    pub restart_required: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupRecord {
    pub id: String,
    pub client_id: String,
    pub target_path: String,
    pub backup_path: String,
    pub created_at: String,
    pub size: u64,
    #[serde(default)]
    pub original_exists: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    pub timeout_seconds: u64,
    pub proxy_url: String,
    pub allow_self_signed_certificates: bool,
    pub generate_only: bool,
    pub clear_clipboard_seconds: u64,
    pub locale: String,
    #[serde(default)]
    pub local_proxy_port: Option<u16>,
    /// 用户显式选择的回退档位。模型自己的推理档位不走这里，见 `ReasoningCapability`。
    #[serde(default)]
    pub manual_reasoning_level: ReasoningLevel,
    /// 由 `manual_reasoning_level` 结算而来，供 `resolve_binding` 作 legacy fallback。
    #[serde(default)]
    pub effective_reasoning_level: ReasoningLevel,
    /// 逐模型兜底档位，优先于全局的 `effective_reasoning_level`。
    ///
    /// 放 AppSettings 而不是 Provider：同一个模型 id 常在多个中转网关下出现，
    /// 用户对"这个模型该用什么档"的判断跟着模型走，不跟着 endpoint 走。
    /// 这与 `ReasoningCapability` 按 `(base_url, model_id)` 索引刻意相反——
    /// 能力是某个端点上的事实，兜底是用户对模型的意图。
    #[serde(default)]
    pub reasoning_fallbacks: Vec<ReasoningFallback>,
    /// 用户自定义的推理档位，供兜底引用。默认空表，程序不预置任何一条。
    #[serde(default)]
    pub custom_reasoning_tiers: Vec<CustomReasoningTier>,
    /// 模型名匹配兜底规则，按数组顺序匹配。默认空表，程序不预置任何一条。
    #[serde(default)]
    pub reasoning_name_rules: Vec<ReasoningNameRule>,
    /// 推理探测投影的时效缓存，按 `(归一化 base_url, model_id)` 索引。
    ///
    /// 这**不是**第二套能力缓存：能力本身的三档 TTL 在 `reasoning_capability.rs`，
    /// 这里只存 `detect_model_reasoning` 那次投影的结果，过期或缺失就按当前能力表重算。
    /// 缓存丢了不影响任何结论，只多一次内存遍历。
    #[serde(default)]
    pub reasoning_detection_cache: Vec<ReasoningDetectionCacheEntry>,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            timeout_seconds: 10,
            proxy_url: String::new(),
            allow_self_signed_certificates: false,
            generate_only: true,
            clear_clipboard_seconds: 30,
            locale: "zh-CN".into(),
            local_proxy_port: None,
            manual_reasoning_level: ReasoningLevel::High,
            effective_reasoning_level: ReasoningLevel::High,
            reasoning_fallbacks: Vec::new(),
            custom_reasoning_tiers: Vec::new(),
            reasoning_name_rules: Vec::new(),
            reasoning_detection_cache: Vec::new(),
        }
    }
}

impl AppSettings {
    /// 查这个模型有没有用户指定的兜底档位，返回它引用的档位 id。
    ///
    /// 全等匹配，绝不做前缀或模糊匹配——见 [`ReasoningFallback`] 第 2 点。
    /// 同一个 model_id 出现多条时取第一条：保存侧已经去重，这里不再报错，
    /// 因为读路径在配置写出的中途，报错会让整个预览失败。
    pub fn reasoning_fallback_for(&self, model_id: &str) -> Option<&str> {
        self.reasoning_fallbacks.iter()
            .find(|item| item.model_id == model_id)
            .map(|item| item.tier_id.as_str())
    }

    /// 按 id 找自定义档位。
    pub fn custom_tier(&self, tier_id: &str) -> Option<&CustomReasoningTier> {
        self.custom_reasoning_tiers.iter().find(|item| item.id == tier_id)
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersistedState {
    pub providers: Vec<Provider>,
    pub backups: Vec<BackupRecord>,
    pub settings: AppSettings,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 自动推荐删除后，旧 state.json 里仍留着 autoReasoningMode 和
    /// reasoningMatchMessage。AppSettings 没有 deny_unknown_fields，这两个键必须被
    /// 静默忽略，且用户已经调好的档位一个都不能丢——升级不许改动用户的既有选择。
    #[test]
    fn legacy_settings_with_removed_reasoning_fields_still_load() {
        let legacy = r#"{
            "timeoutSeconds": 20,
            "proxyUrl": "",
            "allowSelfSignedCertificates": false,
            "generateOnly": true,
            "clearClipboardSeconds": 30,
            "locale": "zh-CN",
            "autoReasoningMode": true,
            "manualReasoningLevel": "low",
            "effectiveReasoningLevel": "medium",
            "reasoningMatchMessage": "云端 API，不占用本机显存，自动选用中度推理模式"
        }"#;

        let settings: AppSettings = serde_json::from_str(legacy).expect("旧 state.json 无法加载");

        assert_eq!(settings.manual_reasoning_level, ReasoningLevel::Low);
        assert_eq!(settings.effective_reasoning_level, ReasoningLevel::Medium);
        assert_eq!(settings.timeout_seconds, 20);
        // 旧文件里没有 reasoningFallbacks，必须默认为空而不是加载失败。
        assert!(settings.reasoning_fallbacks.is_empty());
    }

    /// 兜底表按 camelCase 出入，与前端 types.ts 的 ReasoningFallback 对齐。
    /// 键名写错会让设置静默丢失——序列化一轮能立刻发现。
    #[test]
    fn reasoning_fallbacks_round_trip_in_camel_case() {
        let settings = AppSettings {
            reasoning_fallbacks: vec![ReasoningFallback { model_id: "coder".into(), tier_id: "light".into() }],
            ..AppSettings::default()
        };
        let json = serde_json::to_string(&settings).expect("序列化失败");
        assert!(json.contains("\"reasoningFallbacks\""), "字段名不是 camelCase：{json}");
        assert!(json.contains("\"modelId\":\"coder\""));
        assert!(json.contains("\"tierId\":\"light\""));
        // 迁移完成后不再写出 level，避免落盘里同时存在两个档位来源。
        assert!(!json.contains("\"level\":"), "仍在写出已迁移的 level 字段：{json}");

        let restored: AppSettings = serde_json::from_str(&json).expect("反序列化失败");
        assert_eq!(restored.reasoning_fallbacks, settings.reasoning_fallbacks);
        assert_eq!(restored.reasoning_fallback_for("coder"), Some("light"));
    }

    /// 上一版写出的 `level` 必须平滑读成 tier_id：用户已经设好的兜底不能因为
    /// 字段改名而丢失。low→light / medium→standard / high→deep，与
    /// `ReasoningTier::from_legacy` 同一套映射。
    #[test]
    fn legacy_level_field_migrates_to_tier_id() {
        let legacy = r#"{
            "timeoutSeconds": 10, "proxyUrl": "", "allowSelfSignedCertificates": false,
            "generateOnly": true, "clearClipboardSeconds": 30, "locale": "zh-CN",
            "reasoningFallbacks": [
                {"modelId": "a", "level": "low"},
                {"modelId": "b", "level": "medium"},
                {"modelId": "c", "level": "high"}
            ]
        }"#;
        let settings: AppSettings = serde_json::from_str(legacy).expect("旧兜底表无法加载");
        assert_eq!(settings.reasoning_fallback_for("a"), Some("light"));
        assert_eq!(settings.reasoning_fallback_for("b"), Some("standard"));
        assert_eq!(settings.reasoning_fallback_for("c"), Some("deep"));
    }

    /// 同时存在两个字段时 tierId 胜出：那是新版写出的文件，level 只是残留。
    #[test]
    fn tier_id_wins_over_leftover_level() {
        let both = r#"[{"modelId": "a", "tierId": "max", "level": "low"}]"#;
        let list: Vec<ReasoningFallback> = serde_json::from_str(both).expect("反序列化失败");
        assert_eq!(list[0].tier_id, "max");
    }

    /// 空 model_id 查不到东西。空串是 UI 上"还没填模型名"的那一行，
    /// 它绝不能意外命中所有模型。
    #[test]
    fn empty_model_id_never_matches() {
        let settings = AppSettings {
            reasoning_fallbacks: vec![ReasoningFallback { model_id: String::new(), tier_id: "light".into() }],
            ..AppSettings::default()
        };
        assert_eq!(settings.reasoning_fallback_for("coder"), None);
    }

    /// 默认设置里三张用户表全空。有任何一条内置规则，就等于程序在替用户
    /// 猜模型能力——那是本次改动的红线。
    #[test]
    fn defaults_ship_no_builtin_rules_or_tiers() {
        let settings = AppSettings::default();
        assert!(settings.reasoning_fallbacks.is_empty());
        assert!(settings.custom_reasoning_tiers.is_empty());
        assert!(settings.reasoning_name_rules.is_empty());
    }

    /// 旧 state.json 完全没有这三个键时全部默认为空，行为与旧版本一致。
    #[test]
    fn legacy_settings_without_new_tables_load_as_empty() {
        let legacy = r#"{
            "timeoutSeconds": 20, "proxyUrl": "", "allowSelfSignedCertificates": false,
            "generateOnly": true, "clearClipboardSeconds": 30, "locale": "zh-CN",
            "manualReasoningLevel": "low", "effectiveReasoningLevel": "medium"
        }"#;
        let settings: AppSettings = serde_json::from_str(legacy).expect("旧 state.json 无法加载");
        assert!(settings.custom_reasoning_tiers.is_empty());
        assert!(settings.reasoning_name_rules.is_empty());
        assert!(settings.reasoning_fallbacks.is_empty());
        assert_eq!(settings.manual_reasoning_level, ReasoningLevel::Low);
    }

    /// 新表的序列化契约：camelCase + kebab-case 的 matchType + 三个协议参数键名。
    #[test]
    fn new_tables_round_trip_in_camel_case() {
        let settings = AppSettings {
            custom_reasoning_tiers: vec![CustomReasoningTier {
                id: "tier-1".into(),
                label: "超深".into(),
                description: Some("给自建网关用".into()),
                openai_params: Some(serde_json::json!({"reasoning": {"effort": "xhigh"}})),
                anthropic_params: None,
                gemini_params: None,
            }],
            reasoning_name_rules: vec![ReasoningNameRule {
                id: "rule-1".into(),
                pattern: "glm-".into(),
                match_type: NameMatchType::Prefix,
                tier_id: "tier-1".into(),
            }],
            ..AppSettings::default()
        };
        let json = serde_json::to_string(&settings).expect("序列化失败");
        assert!(json.contains("\"customReasoningTiers\""), "{json}");
        assert!(json.contains("\"reasoningNameRules\""), "{json}");
        assert!(json.contains("\"openaiParams\""), "{json}");
        assert!(json.contains("\"anthropicParams\":null"), "{json}");
        assert!(json.contains("\"matchType\":\"prefix\""), "{json}");

        let restored: AppSettings = serde_json::from_str(&json).expect("反序列化失败");
        assert_eq!(restored.custom_reasoning_tiers, settings.custom_reasoning_tiers);
        assert_eq!(restored.reasoning_name_rules, settings.reasoning_name_rules);
        assert!(restored.custom_tier("tier-1").is_some());
        assert!(restored.custom_tier("missing").is_none());
    }

    /// 旧 state.json 没有 reasoningDetectionCache 键时加载成功，且三张用户表一条不丢。
    ///
    /// 这条比 `legacy_settings_without_new_tables_load_as_empty` 更严：那条验的是空表，
    /// 这条验的是**有内容的旧文件**加载后内容完整——升级不许动用户已经调好的配置。
    #[test]
    fn legacy_settings_without_detection_cache_keep_every_user_table() {
        let legacy = r#"{
            "timeoutSeconds": 20, "proxyUrl": "", "allowSelfSignedCertificates": false,
            "generateOnly": true, "clearClipboardSeconds": 30, "locale": "zh-CN",
            "manualReasoningLevel": "low", "effectiveReasoningLevel": "medium",
            "reasoningFallbacks": [{"modelId": "glm-4-plus", "tierId": "tier-1"}],
            "customReasoningTiers": [{
                "id": "tier-1", "label": "超深", "description": "给自建网关用",
                "openaiParams": {"reasoning": {"effort": "xhigh"}},
                "anthropicParams": {"thinking": {"budget_tokens": 8192}},
                "geminiParams": null
            }],
            "reasoningNameRules": [{"id": "rule-1", "pattern": "glm-", "matchType": "contains", "tierId": "tier-1"}]
        }"#;

        let settings: AppSettings = serde_json::from_str(legacy).expect("旧 state.json 无法加载");

        // 新字段默认为空表，不是加载失败。
        assert!(settings.reasoning_detection_cache.is_empty());
        // 用户的三张表一条不丢，取值也不被改写。
        assert_eq!(settings.reasoning_fallback_for("glm-4-plus"), Some("tier-1"));
        let tier = settings.custom_tier("tier-1").expect("自定义档位丢了");
        assert_eq!(tier.label, "超深");
        assert_eq!(tier.openai_params.as_ref().expect("openai 参数丢了")["reasoning"]["effort"], "xhigh");
        assert_eq!(tier.anthropic_params.as_ref().expect("anthropic 参数丢了")["thinking"]["budget_tokens"], 8192);
        assert!(tier.gemini_params.is_none());
        assert_eq!(settings.reasoning_name_rules.len(), 1);
        assert_eq!(settings.reasoning_name_rules[0].match_type, NameMatchType::Contains);
        assert_eq!(settings.reasoning_name_rules[0].tier_id, "tier-1");
    }

    /// 探测投影结构的序列化契约：camelCase 字段名 + kebab-case 的参数类别 +
    /// 未探明必须能表达成 null 而不是某个具体取值。
    #[test]
    fn detection_meta_serializes_as_camel_case_contract() {
        let meta = ModelReasoningMeta {
            supported_protocols: vec![ProtocolKind::Openai, ProtocolKind::AzureOpenai],
            native_param_kind: NativeParamKind::TokenBudget,
            matched_custom_tiers: vec![MatchedCustomTier {
                tier_id: "tier-1".into(),
                label: "超深".into(),
                rule_pattern: "glm-".into(),
                rule_match_type: NameMatchType::Prefix,
                supported_protocols: vec![ProtocolKind::Anthropic],
            }],
            builtin_tiers_compatible: Some(false),
        };

        let json = serde_json::to_value(&meta).expect("序列化失败");
        assert_eq!(json["nativeParamKind"], "token-budget");
        assert_eq!(json["supportedProtocols"][1], "azure-openai");
        assert_eq!(json["matchedCustomTiers"][0]["tierId"], "tier-1");
        assert_eq!(json["matchedCustomTiers"][0]["rulePattern"], "glm-");
        assert_eq!(json["matchedCustomTiers"][0]["ruleMatchType"], "prefix");
        assert_eq!(json["builtinTiersCompatible"], false);

        // 默认值即"什么都没探明"：类别 unknown、兼容性 null、两张表为空。
        // null 不是 false——把未探明写成不兼容正是本次要修的病。
        let json = serde_json::to_value(ModelReasoningMeta::default()).expect("序列化失败");
        assert_eq!(json["nativeParamKind"], "unknown");
        assert!(json["builtinTiersCompatible"].is_null());
        assert_eq!(json["matchedCustomTiers"].as_array().expect("应为数组").len(), 0);
        assert_eq!(json["supportedProtocols"].as_array().expect("应为数组").len(), 0);

        // 前端传来缺字段的对象也要能反序列化（全字段 serde(default)）。
        let sparse: ModelReasoningMeta = serde_json::from_str("{}").expect("空对象无法反序列化");
        assert_eq!(sparse, ModelReasoningMeta::default());
    }

    /// 档位为哪些协议配了参数，只看它自己填了什么，不看模型名也不看能力表。
    #[test]
    fn tier_reports_only_the_protocols_it_actually_filled() {
        let base = CustomReasoningTier {
            id: "t".into(), label: "t".into(), description: None,
            openai_params: None, anthropic_params: None, gemini_params: None,
        };
        assert!(base.supported_protocols().is_empty());

        // OpenAI 参数同时覆盖 Azure 与 Custom：三者共用同一份形状。
        let openai = CustomReasoningTier { openai_params: Some(serde_json::json!({})), ..base.clone() };
        assert_eq!(
            openai.supported_protocols(),
            vec![ProtocolKind::Openai, ProtocolKind::AzureOpenai, ProtocolKind::Custom]
        );

        let anthropic = CustomReasoningTier { anthropic_params: Some(serde_json::json!({})), ..base.clone() };
        assert_eq!(anthropic.supported_protocols(), vec![ProtocolKind::Anthropic]);

        let gemini = CustomReasoningTier { gemini_params: Some(serde_json::json!({})), ..base };
        assert_eq!(gemini.supported_protocols(), vec![ProtocolKind::Gemini]);
    }

    /// 三个协议参数全空的自定义档位是无效档位——它引用起来必然降级。
    #[test]
    fn a_custom_tier_without_any_params_is_useless() {
        let empty = CustomReasoningTier {
            id: "t".into(), label: "空".into(), description: None,
            openai_params: None, anthropic_params: None, gemini_params: None,
        };
        assert!(!empty.has_any_params());
        assert!(CustomReasoningTier { openai_params: Some(serde_json::json!({})), ..empty }.has_any_params());
    }
}
