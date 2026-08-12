use serde::{Deserialize, Serialize};

use crate::model::ReasoningLevel;

pub const TTL_SUPPORTED_SECONDS: u64 = 14 * 24 * 3600;
pub const TTL_UNSUPPORTED_SECONDS: u64 = 24 * 3600;
pub const TTL_UNKNOWN_SECONDS: u64 = 6 * 3600;
const MAX_EVIDENCE: usize = 8;
const BUDGET_GRID: u64 = 1024;

/// 能力归属键。推理能力属于 (base_url, model_id)，不属于全局设置。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ReasoningKey {
    pub base_url: String,
    pub model_id: String,
}

impl ReasoningKey {
    pub fn new(base_url: impl Into<String>, model_id: impl Into<String>) -> Self {
        Self { base_url: base_url.into(), model_id: model_id.into() }
    }

    pub fn matches(&self, base_url: &str, model_id: &str) -> bool {
        self.base_url == base_url && self.model_id == model_id
    }
}

/// 三态结论。unknown 必须可表达：未探明不等于不支持。
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ReasoningSupport {
    #[default]
    Unknown,
    Unsupported,
    Supported,
}

/// 置信度阶梯，与四级证据阶梯一一对应，可比较大小。
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "lowercase")]
pub enum ReasoningConfidence {
    #[default]
    Unknown,
    Declared,
    Validated,
    Verified,
}

impl ReasoningConfidence {
    pub fn label(self) -> &'static str {
        match self {
            Self::Unknown => "未探明",
            Self::Declared => "服务端声明",
            Self::Validated => "参数校验确认",
            Self::Verified => "真实响应证实",
        }
    }
}

/// 证据来源，与 Tier 阶梯对应。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum EvidenceSource {
    /// Tier 0：模型元数据自述。
    ModelListMetadata,
    /// Tier 1：introspection 端点。
    Introspection,
    /// 旧名，仅为反序列化既有 state.json 保留。新证据一律用 [`Self::CapabilityValidation`]。
    ValidationProbe,
    /// Tier 2：capability validation probe。带自识别头、输出上限 1 token 的受控真实请求。
    CapabilityValidation,
    /// 旧名，已不再产生。曾用于表示"计费真实请求"，该语义由后续 Step 的
    /// user runtime verification 承接。
    BilledProbe,
    /// Tier 4：被动观测线上响应用量。
    RuntimeObservation,
    /// 用户手动覆写。
    ManualOverride,
}

impl EvidenceSource {
    /// 展示用中文标签，供 UI 说明"这条结论从哪来"。
    pub fn label(self) -> &'static str {
        match self {
            Self::ModelListMetadata => "模型元数据",
            Self::Introspection => "能力查询端点",
            Self::ValidationProbe | Self::CapabilityValidation => "能力验证探测",
            Self::BilledProbe => "真实请求验证",
            Self::RuntimeObservation => "线上用量观测",
            Self::ManualOverride => "手动覆写",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ReasoningEvidence {
    pub source: EvidenceSource,
    pub endpoint: Option<String>,
    /// 已脱敏的证据摘要，可直接展示给用户。
    pub detail: String,
    pub observed_at: String,
}

impl ReasoningEvidence {
    pub fn new(source: EvidenceSource, endpoint: Option<String>, detail: impl Into<String>) -> Self {
        Self { source, endpoint, detail: detail.into(), observed_at: chrono::Utc::now().to_rfc3339() }
    }
}

/// 控制形态。这里保留发现到的完整真相，UI 档位只是它的策展视图。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ReasoningControl {
    /// 服务端声明或探测到的 effort 枚举成员，原样保留，绝不发明成员。
    EffortEnum { values: Vec<String> },
    /// 数值预算型（Anthropic budget_tokens / Gemini thinkingBudget）。
    #[serde(rename_all = "camelCase")]
    TokenBudget {
        min: u64,
        max: u64,
        off_allowed: bool,
        /// 交由模型自行分配预算的哨兵值，例如 Gemini 的 -1。
        dynamic_sentinel: Option<i64>,
    },
    /// 只有开关，没有强度维度。
    BooleanToggle,
    None,
}

/// UI 面向的语义档位。跨协议可比较，用于自动推荐与旧配置迁移。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "lowercase")]
pub enum ReasoningTier {
    Off,
    Light,
    Standard,
    Deep,
    Max,
}

impl ReasoningTier {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Light => "light",
            Self::Standard => "standard",
            Self::Deep => "deep",
            Self::Max => "max",
        }
    }

    /// 沿用既有 UI 词汇（轻度／中度／高），避免无谓的用户再学习成本。
    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "关闭",
            Self::Light => "轻度",
            Self::Standard => "中度",
            Self::Deep => "高",
            Self::Max => "最大",
        }
    }

    pub fn from_legacy(level: ReasoningLevel) -> Self {
        match level {
            ReasoningLevel::Low => Self::Light,
            ReasoningLevel::Medium => Self::Standard,
            ReasoningLevel::High => Self::Deep,
        }
    }

    /// 迁移期回写旧字段用。旧枚举只有三档，Off/Light 一并落到 Low。
    pub fn to_legacy(self) -> ReasoningLevel {
        match self {
            Self::Off | Self::Light => ReasoningLevel::Low,
            Self::Standard => ReasoningLevel::Medium,
            Self::Deep | Self::Max => ReasoningLevel::High,
        }
    }
}

/// 档位到线上参数的绑定。Adapter 是唯一把它翻译成协议字段的地方。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ReasoningBinding {
    /// 直接使用服务端认可的 effort 字符串。
    Effort { value: String },
    /// 明确的 token 预算。
    Budget { tokens: u64 },
    /// 交由模型自行分配预算。
    DynamicBudget { sentinel: i64 },
    /// 只开启，不指定强度。
    Enabled,
    /// 显式关闭（budget=0 或 thinking.type=disabled）。
    Disabled,
    /// 不发送任何推理参数，沿用服务端默认。
    Omitted,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ReasoningTierOption {
    pub tier: ReasoningTier,
    /// 稳定标识，UI 用它作为 select 的 value。
    pub id: String,
    pub label: String,
    pub binding: ReasoningBinding,
    /// 展示给用户的实际线上取值，让映射关系可见。
    pub wire_summary: String,
}

/// 协议或模型侧的硬约束，写出端与请求映射都必须遵守。
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ReasoningConstraints {
    /// 预算必须严格小于 max_tokens（Anthropic）。
    #[serde(default)]
    pub budget_below_max_tokens: bool,
    /// 开启推理后 temperature / top_p 受限（Anthropic）。
    #[serde(default)]
    pub locks_sampling_params: bool,
    /// 该模型无法关闭推理（部分 Gemini 2.5 Pro）。
    #[serde(default)]
    pub cannot_disable: bool,
    /// 附加中文说明，直接展示。
    #[serde(default)]
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ReasoningCapability {
    pub key: ReasoningKey,
    pub support: ReasoningSupport,
    pub control: ReasoningControl,
    pub tiers: Vec<ReasoningTierOption>,
    pub default_tier: Option<ReasoningTier>,
    /// 默认档位的中文理由，与既有 reasoningMatchMessage 风格一致。
    pub default_reason: Option<String>,
    pub constraints: ReasoningConstraints,
    pub confidence: ReasoningConfidence,
    pub evidence: Vec<ReasoningEvidence>,
    pub discovered_at: String,
    pub ttl_seconds: u64,
}

impl ReasoningCapability {
    fn base(key: ReasoningKey, support: ReasoningSupport, control: ReasoningControl, ttl_seconds: u64) -> Self {
        Self {
            key,
            support,
            control,
            tiers: Vec::new(),
            default_tier: None,
            default_reason: None,
            constraints: ReasoningConstraints::default(),
            confidence: ReasoningConfidence::Unknown,
            evidence: Vec::new(),
            discovered_at: chrono::Utc::now().to_rfc3339(),
            ttl_seconds,
        }
    }

    /// 未探明。UI 只提供手动覆写，写出端一律省略参数。
    pub fn unknown(key: ReasoningKey) -> Self {
        Self::base(key, ReasoningSupport::Unknown, ReasoningControl::None, TTL_UNKNOWN_SECONDS)
    }

    /// 明确不支持。TTL 较短，网关随时可能升级。
    pub fn unsupported(key: ReasoningKey, confidence: ReasoningConfidence) -> Self {
        let mut value = Self::base(key, ReasoningSupport::Unsupported, ReasoningControl::None, TTL_UNSUPPORTED_SECONDS);
        value.confidence = confidence;
        value
    }

    /// 由发现到的 effort 枚举成员合成档位。成员集合原样保留在 control 中，
    /// tiers 只是它的策展视图；任何未出现在 values 里的取值都不会被发明出来。
    pub fn from_effort_enum(key: ReasoningKey, values: &[String], confidence: ReasoningConfidence) -> Self {
        let members = dedup_preserving_order(values);
        if members.is_empty() { return Self::unknown(key); }
        let (off_members, active) = split_off_members(&members);
        let mut value = Self::base(
            key,
            ReasoningSupport::Supported,
            ReasoningControl::EffortEnum { values: members.clone() },
            TTL_SUPPORTED_SECONDS,
        );
        value.confidence = confidence;
        if let Some(member) = off_members.first() {
            value.tiers.push(effort_option(ReasoningTier::Off, member));
        }
        for (tier, member) in assign_tiers(&active) {
            value.tiers.push(effort_option(tier, member));
        }
        value.apply_default_tier();
        value
    }

    /// 由发现到的预算区间合成档位。区间来自探测，不是内置表。
    pub fn from_token_budget(
        key: ReasoningKey,
        min: u64,
        max: u64,
        off_allowed: bool,
        dynamic_sentinel: Option<i64>,
        confidence: ReasoningConfidence,
    ) -> Self {
        if max == 0 || max < min { return Self::unknown(key); }
        let mut value = Self::base(
            key,
            ReasoningSupport::Supported,
            ReasoningControl::TokenBudget { min, max, off_allowed, dynamic_sentinel },
            TTL_SUPPORTED_SECONDS,
        );
        value.confidence = confidence;
        if off_allowed {
            value.tiers.push(ReasoningTierOption {
                tier: ReasoningTier::Off,
                id: ReasoningTier::Off.as_str().into(),
                label: ReasoningTier::Off.label().into(),
                binding: ReasoningBinding::Disabled,
                wire_summary: "不启用思考预算".into(),
            });
        }
        let mut used = Vec::new();
        for (tier, tokens) in budget_ladder(min, max, off_allowed) {
            if used.contains(&tokens) { continue; }
            used.push(tokens);
            let dynamic = matches!(tier, ReasoningTier::Standard).then_some(dynamic_sentinel).flatten();
            value.tiers.push(match dynamic {
                Some(sentinel) => ReasoningTierOption {
                    tier,
                    id: tier.as_str().into(),
                    label: format!("{}（模型自行分配）", tier.label()),
                    binding: ReasoningBinding::DynamicBudget { sentinel },
                    wire_summary: format!("预算 {sentinel}（自动分配）"),
                },
                None => budget_option(tier, tokens),
            });
        }
        value.apply_default_tier();
        value
    }

    /// 只探到开关、没探到区间时使用。
    pub fn from_boolean_toggle(key: ReasoningKey, off_allowed: bool, confidence: ReasoningConfidence) -> Self {
        let mut value = Self::base(key, ReasoningSupport::Supported, ReasoningControl::BooleanToggle, TTL_SUPPORTED_SECONDS);
        value.confidence = confidence;
        if off_allowed {
            value.tiers.push(ReasoningTierOption {
                tier: ReasoningTier::Off,
                id: ReasoningTier::Off.as_str().into(),
                label: ReasoningTier::Off.label().into(),
                binding: ReasoningBinding::Disabled,
                wire_summary: "关闭思考".into(),
            });
        }
        value.tiers.push(ReasoningTierOption {
            tier: ReasoningTier::Standard,
            id: ReasoningTier::Standard.as_str().into(),
            label: ReasoningTier::Standard.label().into(),
            binding: ReasoningBinding::Enabled,
            wire_summary: "开启思考（未探到可调区间）".into(),
        });
        value.apply_default_tier();
        value
    }
}

impl ReasoningCapability {
    /// 默认优先 Standard，否则取离 Standard 最近且更省的一档；绝不默认选 Off。
    fn apply_default_tier(&mut self) {
        let chosen = self.tiers.iter()
            .filter(|option| option.tier != ReasoningTier::Off)
            .min_by_key(|option| {
                let distance = (option.tier as i32 - ReasoningTier::Standard as i32).abs();
                (distance, option.tier as i32)
            })
            .map(|option| (option.tier, option.label.clone(), option.wire_summary.clone()));
        match chosen {
            Some((tier, label, wire)) => {
                self.default_tier = Some(tier);
                self.default_reason = Some(format!(
                    "{}：共发现 {} 个可用推理档位，默认选用{label}（{wire}）",
                    self.confidence.label(),
                    self.tiers.len(),
                ));
            }
            None => {
                self.default_tier = self.tiers.first().map(|option| option.tier);
                self.default_reason = None;
            }
        }
    }

    pub fn tier(&self, tier: ReasoningTier) -> Option<&ReasoningTierOption> {
        self.tiers.iter().find(|option| option.tier == tier)
    }

    pub fn tier_by_id(&self, id: &str) -> Option<&ReasoningTierOption> {
        self.tiers.iter().find(|option| option.id == id)
    }

    pub fn default_option(&self) -> Option<&ReasoningTierOption> {
        self.default_tier.and_then(|tier| self.tier(tier))
    }

    /// 选不到精确档位时退到语义上最接近的一档，避免用户选择被静默丢弃。
    pub fn nearest_tier(&self, wanted: ReasoningTier) -> Option<&ReasoningTierOption> {
        self.tiers.iter().min_by_key(|option| {
            ((option.tier as i32 - wanted as i32).abs(), option.tier as i32)
        })
    }

    pub fn push_evidence(&mut self, evidence: ReasoningEvidence) {
        if self.evidence.iter().any(|item| item.source == evidence.source && item.detail == evidence.detail) { return; }
        self.evidence.push(evidence);
        if self.evidence.len() > MAX_EVIDENCE {
            let overflow = self.evidence.len() - MAX_EVIDENCE;
            self.evidence.drain(0..overflow);
        }
    }

    pub fn is_stale(&self) -> bool {
        let Ok(discovered) = chrono::DateTime::parse_from_rfc3339(&self.discovered_at) else { return true; };
        let age = chrono::Utc::now().signed_duration_since(discovered.with_timezone(&chrono::Utc));
        age.num_seconds() < 0 || age.num_seconds() as u64 >= self.ttl_seconds
    }

    /// 是否需要重新发现。**只看 TTL**。
    ///
    /// 这里曾经额外短路 `confidence == Unknown`，导致"未探明"永远不入缓存：每次
    /// save_provider 都会重发一次 Tier 2 探测，没有任何退避。未探明同样是一个需要
    /// 缓存的结论（TTL 6 小时），退避窗口由 [`TTL_UNKNOWN_SECONDS`] 表达。
    pub fn should_rediscover(&self) -> bool {
        self.is_stale()
    }

    /// 合并新证据。置信度不低于现状（或现状已过期）才替换结论，证据始终累积。
    /// 键不一致直接拒绝，防止换 base_url 后误用旧缓存。
    pub fn merge(&mut self, incoming: ReasoningCapability) -> bool {
        if self.key != incoming.key { return false; }
        if incoming.confidence >= self.confidence || self.is_stale() {
            let carried = std::mem::take(&mut self.evidence);
            *self = incoming;
            // 旧证据先入、新证据后入：超出上限时优先丢弃最旧的一条。
            let fresh = std::mem::take(&mut self.evidence);
            for evidence in carried.into_iter().chain(fresh) { self.push_evidence(evidence); }
        } else {
            for evidence in incoming.evidence { self.push_evidence(evidence); }
        }
        true
    }
}

fn dedup_preserving_order(values: &[String]) -> Vec<String> {
    let mut result: Vec<String> = Vec::new();
    for value in values {
        let trimmed = value.trim();
        if trimmed.is_empty() { continue; }
        if result.iter().any(|item| item.eq_ignore_ascii_case(trimmed)) { continue; }
        result.push(trimmed.to_owned());
    }
    result
}

/// 参数取值词表的强弱序，不是模型清单：成员本身必须来自服务端发现，
/// 这里只负责给已发现的成员排序。未收录的词汇按发现顺序处理。
fn effort_rank(value: &str) -> Option<u8> {
    match value.trim().to_ascii_lowercase().as_str() {
        "none" | "off" | "disabled" => Some(0),
        "minimal" => Some(1),
        "low" => Some(2),
        "medium" | "med" => Some(3),
        "high" => Some(4),
        "xhigh" | "x-high" | "very-high" => Some(5),
        "max" | "maximum" => Some(6),
        _ => None,
    }
}

fn split_off_members(members: &[String]) -> (Vec<String>, Vec<String>) {
    let (off, mut active): (Vec<String>, Vec<String>) = members.iter().cloned()
        .partition(|value| effort_rank(value) == Some(0));
    if active.iter().all(|value| effort_rank(value).is_some()) {
        active.sort_by_key(|value| effort_rank(value).unwrap_or(u8::MAX));
    }
    (off, active)
}

fn tier_for_rank(rank: u8) -> ReasoningTier {
    match rank {
        0 => ReasoningTier::Off,
        1 | 2 => ReasoningTier::Light,
        3 => ReasoningTier::Standard,
        4 => ReasoningTier::Deep,
        _ => ReasoningTier::Max,
    }
}

/// 词表全部可识别且不发生档位碰撞时按语义映射；否则按位置均匀铺开。
/// 两条路径都只在已发现成员内取值。
fn assign_tiers(active: &[String]) -> Vec<(ReasoningTier, &String)> {
    if active.is_empty() { return Vec::new(); }
    let ranked: Option<Vec<ReasoningTier>> = active.iter()
        .map(|value| effort_rank(value).map(tier_for_rank))
        .collect();
    if let Some(tiers) = ranked {
        let unique = tiers.iter().collect::<std::collections::BTreeSet<_>>().len();
        if unique == tiers.len() {
            return tiers.into_iter().zip(active.iter()).collect();
        }
    }
    spread_tiers(active.len()).into_iter().zip(pick_representatives(active)).collect()
}

fn spread_tiers(count: usize) -> Vec<ReasoningTier> {
    use ReasoningTier::{Deep, Light, Max, Standard};
    match count {
        0 => vec![],
        1 => vec![Standard],
        2 => vec![Light, Deep],
        3 => vec![Light, Standard, Deep],
        _ => vec![Light, Standard, Deep, Max],
    }
}

/// 成员多于四个时取四个等距代表，其余成员仍保留在 control 中供手动覆写。
fn pick_representatives(active: &[String]) -> Vec<&String> {
    let count = active.len();
    if count <= 4 { return active.iter().collect(); }
    (0..4).map(|slot| &active[slot * (count - 1) / 3]).collect()
}

fn round_to_grid(value: u64, min: u64, max: u64) -> u64 {
    let rounded = ((value + BUDGET_GRID / 2) / BUDGET_GRID) * BUDGET_GRID;
    rounded.clamp(min.max(1), max)
}

/// 端点取发现到的真实边界，中间两档按区间比例落到 1024 网格上。
fn budget_ladder(min: u64, max: u64, off_allowed: bool) -> Vec<(ReasoningTier, u64)> {
    let floor = if min == 0 && off_allowed { BUDGET_GRID.min(max) } else { min };
    let span = max.saturating_sub(floor);
    vec![
        (ReasoningTier::Light, floor.max(1).min(max)),
        (ReasoningTier::Standard, round_to_grid(floor + span / 5, floor.max(1), max)),
        (ReasoningTier::Deep, round_to_grid(floor + span * 3 / 5, floor.max(1), max)),
        (ReasoningTier::Max, max),
    ]
}

fn effort_option(tier: ReasoningTier, value: &str) -> ReasoningTierOption {
    ReasoningTierOption {
        tier,
        id: tier.as_str().into(),
        label: format!("{}（{value}）", tier.label()),
        binding: ReasoningBinding::Effort { value: value.to_owned() },
        wire_summary: format!("effort = {value}"),
    }
}

fn budget_option(tier: ReasoningTier, tokens: u64) -> ReasoningTierOption {
    ReasoningTierOption {
        tier,
        id: tier.as_str().into(),
        label: format!("{}（{tokens} tokens）", tier.label()),
        binding: ReasoningBinding::Budget { tokens },
        wire_summary: format!("预算 {tokens} tokens"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_has_no_tiers() {
        let cap = ReasoningCapability::unknown(ReasoningKey::new("https://api.example.com/v1", "model"));
        assert_eq!(cap.support, ReasoningSupport::Unknown);
        assert!(cap.tiers.is_empty());
        assert!(cap.default_tier.is_none());
    }

    #[test]
    fn openai_style_enum_four_members() {
        let cap = ReasoningCapability::from_effort_enum(
            ReasoningKey::new("https://api.openai.com/v1", "o1"),
            &["low".into(), "medium".into(), "high".into(), "max".into()],
            ReasoningConfidence::Declared,
        );
        assert_eq!(cap.support, ReasoningSupport::Supported);
        assert_eq!(cap.tiers.len(), 4);
        assert_eq!(cap.tiers[0].tier, ReasoningTier::Light);
        assert_eq!(cap.tiers[1].tier, ReasoningTier::Standard);
        assert_eq!(cap.tiers[2].tier, ReasoningTier::Deep);
        assert_eq!(cap.tiers[3].tier, ReasoningTier::Max);
        assert_eq!(cap.default_tier, Some(ReasoningTier::Standard));
        if let ReasoningBinding::Effort { value } = &cap.tiers[1].binding {
            assert_eq!(value, "medium");
        } else {
            panic!("Expected Effort binding");
        }
    }

    #[test]
    fn openai_minimal_becomes_light() {
        let cap = ReasoningCapability::from_effort_enum(
            ReasoningKey::new("https://api.openai.com/v1", "o1-mini"),
            &["minimal".into(), "low".into(), "high".into()],
            ReasoningConfidence::Validated,
        );
        assert_eq!(cap.tiers.len(), 3);
        assert_eq!(cap.tiers[0].tier, ReasoningTier::Light);
        assert_eq!(cap.tiers[1].tier, ReasoningTier::Standard);
        assert_eq!(cap.tiers[2].tier, ReasoningTier::Deep);
    }

    #[test]
    fn anthropic_budget_ladder_omits_duplicates() {
        let cap = ReasoningCapability::from_token_budget(
            ReasoningKey::new("https://api.anthropic.com/v1", "claude-sonnet-4-20250514"),
            1024,
            2048,
            false,
            None,
            ReasoningConfidence::Validated,
        );
        assert_eq!(cap.support, ReasoningSupport::Supported);
        assert!(cap.tiers.len() <= 4);
        for option in &cap.tiers {
            assert!(option.tier != ReasoningTier::Off);
            if let ReasoningBinding::Budget { tokens } = option.binding {
                assert!(tokens >= 1024 && tokens <= 2048);
            } else {
                panic!("Expected Budget binding");
            }
        }
    }

    #[test]
    fn gemini_dynamic_sentinel_goes_to_standard() {
        let cap = ReasoningCapability::from_token_budget(
            ReasoningKey::new("https://generativelanguage.googleapis.com/v1beta", "gemini-2.0-flash-thinking-exp"),
            0,
            8192,
            true,
            Some(-1),
            ReasoningConfidence::Declared,
        );
        let standard = cap.tier(ReasoningTier::Standard).expect("Standard tier missing");
        assert!(matches!(standard.binding, ReasoningBinding::DynamicBudget { sentinel: -1 }));
        assert!(standard.wire_summary.contains("自动分配"));
    }

    #[test]
    fn boolean_toggle_has_two_tiers() {
        let cap = ReasoningCapability::from_boolean_toggle(
            ReasoningKey::new("https://api.anthropic.com/v1", "claude-3-5-sonnet-20241022"),
            true,
            ReasoningConfidence::Validated,
        );
        assert_eq!(cap.tiers.len(), 2);
        assert_eq!(cap.tiers[0].tier, ReasoningTier::Off);
        assert_eq!(cap.tiers[1].tier, ReasoningTier::Standard);
    }

    #[test]
    fn legacy_migration() {
        assert_eq!(ReasoningTier::from_legacy(ReasoningLevel::Low), ReasoningTier::Light);
        assert_eq!(ReasoningTier::from_legacy(ReasoningLevel::Medium), ReasoningTier::Standard);
        assert_eq!(ReasoningTier::from_legacy(ReasoningLevel::High), ReasoningTier::Deep);
        assert_eq!(ReasoningTier::Off.to_legacy(), ReasoningLevel::Low);
        assert_eq!(ReasoningTier::Max.to_legacy(), ReasoningLevel::High);
    }

    #[test]
    fn stale_detection() {
        let mut cap = ReasoningCapability::unknown(ReasoningKey::new("https://api.example.com/v1", "model"));
        cap.discovered_at = "2020-01-01T00:00:00Z".into();
        assert!(cap.is_stale());
    }

    #[test]
    fn merge_keeps_higher_confidence() {
        let mut base = ReasoningCapability::unknown(ReasoningKey::new("https://api.example.com/v1", "model"));
        base.confidence = ReasoningConfidence::Declared;
        let incoming = ReasoningCapability::from_effort_enum(
            ReasoningKey::new("https://api.example.com/v1", "model"),
            &["low".into(), "high".into()],
            ReasoningConfidence::Validated,
        );
        base.merge(incoming);
        assert_eq!(base.confidence, ReasoningConfidence::Validated);
        assert_eq!(base.tiers.len(), 2);
    }

    #[test]
    fn merge_rejects_different_key() {
        let mut base = ReasoningCapability::unknown(ReasoningKey::new("https://api.example.com/v1", "model-a"));
        let incoming = ReasoningCapability::unknown(ReasoningKey::new("https://api.example.com/v1", "model-b"));
        assert!(!base.merge(incoming));
    }

    #[test]
    fn evidence_deduplication() {
        let mut cap = ReasoningCapability::unknown(ReasoningKey::new("https://api.example.com/v1", "model"));
        cap.push_evidence(ReasoningEvidence::new(EvidenceSource::ModelListMetadata, None, "found field"));
        cap.push_evidence(ReasoningEvidence::new(EvidenceSource::ModelListMetadata, None, "found field"));
        assert_eq!(cap.evidence.len(), 1);
    }

    #[test]
    fn off_member_goes_to_off_tier() {
        let cap = ReasoningCapability::from_effort_enum(
            ReasoningKey::new("https://api.example.com/v1", "model"),
            &["off".into(), "low".into(), "high".into()],
            ReasoningConfidence::Declared,
        );
        assert_eq!(cap.tiers[0].tier, ReasoningTier::Off);
        assert_eq!(cap.default_tier, Some(ReasoningTier::Light));
    }

    #[test]
    fn dedup_keeps_first_occurrence() {
        let values = vec!["low".into(), "medium".into(), "LOW".into(), "high".into()];
        let result = dedup_preserving_order(&values);
        assert_eq!(result, vec!["low", "medium", "high"]);
    }

    #[test]
    fn spread_with_two_members() {
        let cap = ReasoningCapability::from_effort_enum(
            ReasoningKey::new("https://api.example.com/v1", "model"),
            &["a".into(), "b".into()],
            ReasoningConfidence::Declared,
        );
        assert_eq!(cap.tiers.len(), 2);
        assert_eq!(cap.tiers[0].tier, ReasoningTier::Light);
        assert_eq!(cap.tiers[1].tier, ReasoningTier::Deep);
    }
}
