//! 用户对推理档位的**选择**。
//!
//! 与 [`crate::reasoning_capability`] 的分工是本模块存在的全部理由：
//!
//! - `ReasoningCapability` = 发现到的**事实**，归属 `(base_url, model_id)`，换端点即失效。
//! - `ReasoningSelection`  = 用户的**意图**，归属 `(provider, model_id)`，换端点仍然有效。
//!
//! 两者生命周期不同，所以不能放在同一个结构里：能力挂在 `ModelInfo.reasoning`（随模型列表
//! 整体替换），选择挂在 `Provider.reasoning_selections`（模型刷新只做剪枝）。
//!
//! 选择只记录**语义档位**而不记录解析后的线上取值：用户选"高"，之后重新发现把预算上限
//! 调大了，"高"应当自动跟到新值——意图是"高"，不是"8192"。确实要钉死某个具体取值时用
//! [`ReasoningSelection::explicit_binding`]。

use serde::{Deserialize, Serialize};

use crate::model::ReasoningLevel;
use crate::reasoning_capability::{ReasoningBinding, ReasoningCapability, ReasoningSupport, ReasoningTier};

/// 这条选择是怎么来的。用枚举而不是 bool，Step 5 的 runtime verification
/// 只需要加一个变体，不改结构。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SelectionSource {
    /// 用户在界面上明确选的。
    User,
    /// 由旧的全局 `manual_reasoning_level` 落到本模型上的隐式回退。
    LegacyFallback,
    /// 采纳了能力表自带的默认档。
    CapabilityDefault,
}

/// 单个模型的推理档位选择。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReasoningSelection {
    pub model_id: String,
    /// 语义档位，权威字段。能力表重新发现后按它重新解析绑定。
    #[serde(default)]
    pub tier: Option<ReasoningTier>,
    /// 高级覆写：用户直接指定的线上取值，优先于 `tier`。
    /// 字段名不用 `override`（Rust 保留字）。
    #[serde(default)]
    pub explicit_binding: Option<ReasoningBinding>,
    pub source: SelectionSource,
    pub chosen_at: String,
}

impl ReasoningSelection {
    pub fn new(model_id: impl Into<String>, tier: ReasoningTier, source: SelectionSource) -> Self {
        Self {
            model_id: model_id.into(),
            tier: Some(tier),
            explicit_binding: None,
            source,
            chosen_at: chrono::Utc::now().to_rfc3339(),
        }
    }

    pub fn with_binding(model_id: impl Into<String>, binding: ReasoningBinding, source: SelectionSource) -> Self {
        Self {
            model_id: model_id.into(),
            tier: None,
            explicit_binding: Some(binding),
            source,
            chosen_at: chrono::Utc::now().to_rfc3339(),
        }
    }
}

/// 解析结果。除了绑定本身，还带上"为什么是这个绑定"——写出端需要它来解释
/// 省略参数的原因，UI 也需要它来显示当前生效档位。
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedReasoning {
    pub binding: ReasoningBinding,
    /// 命中的语义档位。`explicit_binding` 覆写时为 None（用户绕过了档位语义）。
    pub tier: Option<ReasoningTier>,
    /// 中文说明，可直接展示或写进 debug note。
    pub reason: String,
}

impl ResolvedReasoning {
    fn omitted(reason: impl Into<String>) -> Self {
        Self { binding: ReasoningBinding::Omitted, tier: None, reason: reason.into() }
    }

    /// 是否需要向请求/配置写入推理参数。
    pub fn is_omitted(&self) -> bool {
        matches!(self.binding, ReasoningBinding::Omitted)
    }
}

/// 在选择列表里找某个模型的选择。
pub fn selection_for<'a>(selections: &'a [ReasoningSelection], model_id: &str) -> Option<&'a ReasoningSelection> {
    selections.iter().find(|item| item.model_id == model_id)
}

/// 把一条选择合并进列表：同 `model_id` 覆盖，否则追加。
pub fn upsert(selections: &mut Vec<ReasoningSelection>, incoming: ReasoningSelection) {
    match selections.iter_mut().find(|item| item.model_id == incoming.model_id) {
        Some(existing) => *existing = incoming,
        None => selections.push(incoming),
    }
}

/// 把草稿里的选择合并进已存的选择：草稿优先，草稿未提到的模型保留原值。
///
/// 前端可能只提交当前正在编辑的那个模型，不能因此清掉其他模型的选择。
pub fn merge_drafted(existing: &[ReasoningSelection], drafted: &[ReasoningSelection]) -> Vec<ReasoningSelection> {
    let mut merged = existing.to_vec();
    for selection in drafted {
        upsert(&mut merged, selection.clone());
    }
    merged
}

/// 剪掉已经不在模型列表里的选择。
///
/// 只按 `model_id` 剪枝，**不看 base_url**：选择是用户意图，换端点后"我要深度推理"
/// 依然成立，只是需要在新端点的能力表里重新解析。这与
/// [`crate::carry_reasoning_forward`] 对能力的严格 key 校验是刻意的不对称。
pub fn prune_missing(selections: &mut Vec<ReasoningSelection>, models: &[crate::model::ModelInfo]) {
    selections.retain(|selection| models.iter().any(|model| model.id == selection.model_id));
}

/// 把（能力，选择，旧全局设置）解析成一个确定的绑定。
///
/// 优先级：
/// 1. `selection.explicit_binding` —— 用户钉死的线上取值，原样使用
/// 2. `selection.tier` —— 精确命中，失配则退到语义最近一档（不静默丢弃用户选择）
/// 3. 旧全局 `manual_reasoning_level` 的隐式回退
/// 4. 能力表自带的默认档
/// 5. `Omitted` —— 不发送任何推理参数
///
/// 第 3 步压在能力表默认档**之上**是刻意的：`Provider.reasoning_selections` 对老
/// `state.json` 是空数组，此时唯一能代表用户意图的就是旧的全局档位。让能力表默认档
/// 抢在它前面，等于升级一次版本就悄悄改掉老用户已经调好的档位。
/// 能力不支持、未探明、无可用档位这三种情况一律 `Omitted`，绝不猜测取值。
pub fn resolve_binding(
    capability: Option<&ReasoningCapability>,
    selection: Option<&ReasoningSelection>,
    legacy_fallback: Option<ReasoningLevel>,
) -> ResolvedReasoning {
    if let Some(binding) = selection.and_then(|item| item.explicit_binding.clone()) {
        return ResolvedReasoning {
            binding,
            tier: None,
            reason: "用户直接指定了线上取值".into(),
        };
    }

    let Some(capability) = capability else {
        return ResolvedReasoning::omitted("该模型尚未探明推理能力，省略推理参数");
    };

    match capability.support {
        ReasoningSupport::Unsupported => {
            return ResolvedReasoning::omitted(format!(
                "{}：该模型不支持推理，省略推理参数",
                capability.confidence.label()
            ));
        }
        ReasoningSupport::Unknown => {
            return ResolvedReasoning::omitted("该模型尚未探明推理能力，省略推理参数");
        }
        ReasoningSupport::Supported => {}
    }

    if capability.tiers.is_empty() {
        return ResolvedReasoning::omitted("能力已确认但未发现可用档位，省略推理参数");
    }

    if let Some(wanted) = selection.and_then(|item| item.tier) {
        if let Some(option) = capability.tier(wanted) {
            return ResolvedReasoning {
                binding: option.binding.clone(),
                tier: Some(option.tier),
                reason: format!("用户选择{}（{}）", option.label, option.wire_summary),
            };
        }
        if let Some(option) = capability.nearest_tier(wanted) {
            return ResolvedReasoning {
                binding: option.binding.clone(),
                tier: Some(option.tier),
                reason: format!(
                    "用户选择的{}档位在当前能力表中不存在，退到最接近的{}（{}）",
                    wanted.label(),
                    option.label,
                    option.wire_summary
                ),
            };
        }
    }

    if let Some(level) = legacy_fallback {
        let wanted = ReasoningTier::from_legacy(level);
        if let Some(option) = capability.tier(wanted).or_else(|| capability.nearest_tier(wanted)) {
            return ResolvedReasoning {
                binding: option.binding.clone(),
                tier: Some(option.tier),
                reason: format!(
                    "沿用旧的全局手动档位{}，解析为{}（{}）",
                    wanted.label(),
                    option.label,
                    option.wire_summary
                ),
            };
        }
    }

    if let Some(option) = capability.default_option() {
        return ResolvedReasoning {
            binding: option.binding.clone(),
            tier: Some(option.tier),
            reason: capability
                .default_reason
                .clone()
                .unwrap_or_else(|| format!("采用能力表默认档位{}（{}）", option.label, option.wire_summary)),
        };
    }

    ResolvedReasoning::omitted("能力表未提供默认档位，省略推理参数")
}

// —— 兜底档位的结算。**只服务于配置文件写出**。
//
// 这一段与上面的 `resolve_binding` 严格分开，是本任务最重要的一条边界：
//
// - `resolve_binding` 决定**实时请求**发什么。未探明能力一律 `Omitted`——给网关发一个
//   它不认的推理参数会换来 400，而"不发"永远是安全的。这个逻辑本任务一行都不改。
// - 下面两个函数只决定**写进客户端配置文件**的参数。配置文件是给 Claude Code / Codex
//   这类客户端读的，写错了顶多是那个客户端自己报错，用户改一下即可，不会打断本程序。
//
// 两条链路允许给出不同答案，正是因为代价不同。把兜底接到 `resolve_binding` 上省一点
// 代码，换来的是网关 400——那不是省，是把用户的请求当试验品。

/// 把一个档位 id 解析成**当前协议**的原生推理参数。
///
/// 查找顺序：先内置档位，再用户自定义档位。两者都找不到返回 `None`。
///
/// 内置档位只表达 OpenAI 系的 `reasoning.effort`——这不是偷懒。Anthropic 的
/// `budget_tokens` 和 Gemini 的 `thinkingBudget` 是具体数字，程序凭空编一个数字就是
/// 发明取值；用户自己的网关认哪个数字只有用户知道。所以这两个协议的兜底参数只能来自
/// 自定义档位。内置档位在这两个协议下返回 `None`，让结算继续往下降级。
///
/// 返回 `None` 的三种情形调用方一视同仁地当作"这一级不可用，继续往下找"：
/// id 不认识（档位被删了）、自定义档位这个协议没填参数、内置档位遇上非 OpenAI 协议。
pub fn resolve_tier_config(
    tier_id: &str,
    settings: &crate::model::AppSettings,
    protocol: crate::model::ProtocolKind,
) -> Option<serde_json::Value> {
    use crate::model::ProtocolKind;

    let tier_id = tier_id.trim();
    if tier_id.is_empty() {
        return None;
    }

    if let Some(builtin) = ReasoningTier::from_id(tier_id) {
        // Off 档在配置文件里没有可写的表达：注入 `effort: "off"` 是编造取值——
        // 没有哪家网关声明过这个值。要真正关闭推理就该不写参数，正是 None 的含义。
        if builtin == ReasoningTier::Off {
            return None;
        }
        return match protocol {
            ProtocolKind::Openai | ProtocolKind::AzureOpenai | ProtocolKind::Custom => {
                Some(serde_json::json!({ "reasoning": { "effort": builtin.to_legacy().as_str() } }))
            }
            ProtocolKind::Anthropic | ProtocolKind::Gemini => None,
        };
    }

    let custom = settings.custom_tier(tier_id)?;
    let params = match protocol {
        ProtocolKind::Openai | ProtocolKind::AzureOpenai | ProtocolKind::Custom => custom.openai_params.as_ref(),
        ProtocolKind::Anthropic => custom.anthropic_params.as_ref(),
        ProtocolKind::Gemini => custom.gemini_params.as_ref(),
    }?;
    // 原样克隆。本项目不维护各家网关的参数字典，不校验字段名也不改写取值——
    // 一旦校验就等于替用户判断哪些参数合法，而那正是该由网关回答的事。
    Some(params.clone())
}

/// 首个命中的模型名规则。
///
/// 这**不是**「按模型名推断能力」。差别在证据来源：程序不预置任何规则，`reasoning_name_rules`
/// 初始为空，每一条都是用户自己写下的；而且命中结果只影响配置文件写出，不影响探测、
/// 验证和实时请求。用户写下"我的网关里 glm- 开头的都支持推理"是他自己的判断，
/// 程序照办；程序自己下这个判断才是编造事实。
///
/// 匹配规则：数组顺序即优先级，第一条命中的生效；大小写不敏感；空 pattern 不参与匹配
/// （否则它会命中一切模型）。不做正则——正则的失败模式是静默匹配错，而用户在设置页
/// 没有任何调试手段。
pub fn match_name_fallback<'a>(
    model_id: &str,
    rules: &'a [crate::model::ReasoningNameRule],
) -> Option<&'a crate::model::ReasoningNameRule> {
    use crate::model::NameMatchType;

    let model_id = model_id.trim().to_lowercase();
    if model_id.is_empty() {
        return None;
    }
    rules.iter().find(|rule| {
        let pattern = rule.pattern.trim().to_lowercase();
        if pattern.is_empty() {
            return false;
        }
        match rule.match_type {
            NameMatchType::Prefix => model_id.starts_with(&pattern),
            NameMatchType::Contains => model_id.contains(&pattern),
        }
    })
}

/// 配置写出时兜底档位的来源，用于预览里说清这个参数是哪一级给的。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FallbackOrigin {
    /// 用户为这一个模型单独设定的档位。
    ModelFallback,
    /// 命中了用户写的模型名规则。
    NameRule,
}

impl FallbackOrigin {
    /// 预览里的来源标签。措辞全是设定性的——不含"支持""兼容""已确认"，
    /// 那些词属于探测结论，套在用户自己填的值上就是把设定伪装成事实。
    pub fn label(self) -> &'static str {
        match self {
            Self::ModelFallback => "单模型兜底",
            Self::NameRule => "名称规则兜底",
        }
    }
}

/// 兜底结算的结果：写什么参数、这个参数出自哪一级、引用的是哪个档位。
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedFallback {
    pub params: serde_json::Value,
    pub origin: FallbackOrigin,
    pub tier_id: String,
}

/// 未探明能力的模型该按哪一级兜底写出参数。
///
/// 优先级：单模型兜底 > 模型名规则兜底。两级都不可用时返回 `None`，由调用方接着走
/// 全局手动回退档（那一级不经过档位表，是旧枚举直接映射）。
///
/// 「不可用」包含四种情况，全部平滑降级到下一级，绝不报错、绝不中断写入：
/// 规则不命中、引用的档位已被删除、档位存在但当前协议没有参数、Off 档（无可写表达）。
///
/// 单模型兜底压过名称规则：前者点名了这一个模型，后者是一条泛化规则，
/// 具体的意图应当胜过宽泛的意图。
pub fn resolve_fallback_params(
    model_id: &str,
    settings: &crate::model::AppSettings,
    protocol: crate::model::ProtocolKind,
) -> Option<ResolvedFallback> {
    if let Some(tier_id) = settings.reasoning_fallback_for(model_id) {
        if let Some(params) = resolve_tier_config(tier_id, settings, protocol) {
            return Some(ResolvedFallback {
                params,
                origin: FallbackOrigin::ModelFallback,
                tier_id: tier_id.to_owned(),
            });
        }
    }

    // 逐条往下试，而不是只看首个命中的规则：首条规则引用的档位可能已被删除，
    // 此时"用户还写了另一条也能命中的规则"显然比"直接掉到全局档"更接近他的意图。
    let target = model_id.trim().to_lowercase();
    if target.is_empty() {
        return None;
    }
    let mut rest = settings.reasoning_name_rules.as_slice();
    while let Some(rule) = match_name_fallback(model_id, rest) {
        if let Some(params) = resolve_tier_config(&rule.tier_id, settings, protocol) {
            return Some(ResolvedFallback {
                params,
                origin: FallbackOrigin::NameRule,
                tier_id: rule.tier_id.clone(),
            });
        }
        let consumed = rest.iter().position(|item| std::ptr::eq(item, rule)).map_or(rest.len(), |at| at + 1);
        rest = &rest[consumed..];
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ModelInfo, ProtocolKind};
    use crate::reasoning_capability::{ReasoningConfidence, ReasoningKey};

    fn key() -> ReasoningKey {
        ReasoningKey::new("https://api.example.com/v1", "m")
    }

    fn effort_capability() -> ReasoningCapability {
        ReasoningCapability::from_effort_enum(
            key(),
            &["low".into(), "medium".into(), "high".into()],
            ReasoningConfidence::Validated,
        )
    }

    fn budget_capability() -> ReasoningCapability {
        ReasoningCapability::from_token_budget(key(), 1024, 8192, true, None, ReasoningConfidence::Validated)
    }

    fn model(id: &str) -> ModelInfo {
        ModelInfo {
            id: id.into(),
            display_name: id.into(),
            provider: None,
            protocol: ProtocolKind::Openai,
            source: "test".into(),
            capabilities: Vec::new(),
            context_window: None,
            parameter_count_billions: None,
            reasoning: None,
        }
    }

    /// 优先级 1：显式绑定原样使用，连能力表都不查——用户钉死了取值。
    #[test]
    fn explicit_binding_wins_over_everything() {
        let selection = ReasoningSelection::with_binding(
            "m",
            ReasoningBinding::Budget { tokens: 8000 },
            SelectionSource::User,
        );
        let resolved = resolve_binding(Some(&effort_capability()), Some(&selection), Some(ReasoningLevel::Low));
        assert_eq!(resolved.binding, ReasoningBinding::Budget { tokens: 8000 });
        assert_eq!(resolved.tier, None);
    }

    /// 优先级 2：档位精确命中。
    #[test]
    fn selected_tier_resolves_exactly() {
        let selection = ReasoningSelection::new("m", ReasoningTier::Deep, SelectionSource::User);
        let resolved = resolve_binding(Some(&effort_capability()), Some(&selection), None);
        assert_eq!(resolved.tier, Some(ReasoningTier::Deep));
        assert_eq!(resolved.binding, ReasoningBinding::Effort { value: "high".into() });
    }

    /// 优先级 2 的兜底：档位表变了，用户选择不能被静默丢弃。
    #[test]
    fn missing_tier_falls_back_to_nearest_instead_of_being_dropped() {
        // 只有 low/high 两档，用户此前选的 Max 不存在。
        let capability = ReasoningCapability::from_effort_enum(
            key(),
            &["low".into(), "high".into()],
            ReasoningConfidence::Declared,
        );
        let selection = ReasoningSelection::new("m", ReasoningTier::Max, SelectionSource::User);
        let resolved = resolve_binding(Some(&capability), Some(&selection), None);
        assert!(resolved.tier.is_some(), "用户选择被丢弃了");
        assert!(!resolved.is_omitted());
        assert!(resolved.reason.contains("最接近"), "未说明发生了近似，实际：{}", resolved.reason);
    }

    /// 优先级 3：没有任何选择时采用能力表默认档。
    #[test]
    fn no_selection_uses_capability_default() {
        let capability = effort_capability();
        let resolved = resolve_binding(Some(&capability), None, None);
        assert_eq!(resolved.tier, capability.default_tier);
        assert!(!resolved.is_omitted());
    }

    /// 优先级 4：旧全局档位在无显式选择时生效，且优先于能力表默认档——
    /// 老用户升级后行为不能突变。
    #[test]
    fn legacy_level_applies_when_no_selection_exists() {
        let resolved = resolve_binding(Some(&budget_capability()), None, Some(ReasoningLevel::Low));
        assert_eq!(resolved.tier, Some(ReasoningTier::Light));
        assert!(resolved.reason.contains("旧的全局手动档位"), "实际：{}", resolved.reason);
    }

    /// 显式选择必须压过旧全局值，否则新 UI 的选择会被老配置吃掉。
    #[test]
    fn selection_overrides_legacy_level() {
        let selection = ReasoningSelection::new("m", ReasoningTier::Deep, SelectionSource::User);
        let resolved = resolve_binding(Some(&effort_capability()), Some(&selection), Some(ReasoningLevel::Low));
        assert_eq!(resolved.tier, Some(ReasoningTier::Deep));
    }

    /// 优先级 5：三种"没有可用档位"的情形一律省略参数，绝不猜。
    #[test]
    fn unknown_unsupported_and_absent_all_omit() {
        let selection = ReasoningSelection::new("m", ReasoningTier::Deep, SelectionSource::User);

        let absent = resolve_binding(None, Some(&selection), Some(ReasoningLevel::High));
        assert!(absent.is_omitted());

        let unknown = ReasoningCapability::unknown(key());
        assert!(resolve_binding(Some(&unknown), Some(&selection), Some(ReasoningLevel::High)).is_omitted());

        let unsupported = ReasoningCapability::unsupported(key(), ReasoningConfidence::Validated);
        let resolved = resolve_binding(Some(&unsupported), Some(&selection), Some(ReasoningLevel::High));
        assert!(resolved.is_omitted());
        assert!(resolved.reason.contains("不支持"), "实际：{}", resolved.reason);
    }

    /// 草稿只提交了一个模型时，其他模型的既有选择必须保留。
    #[test]
    fn merging_a_draft_keeps_untouched_models() {
        let existing = vec![
            ReasoningSelection::new("model-a", ReasoningTier::Light, SelectionSource::User),
            ReasoningSelection::new("model-b", ReasoningTier::Deep, SelectionSource::User),
        ];
        let drafted = vec![ReasoningSelection::new("model-a", ReasoningTier::Max, SelectionSource::User)];

        let merged = merge_drafted(&existing, &drafted);

        assert_eq!(merged.len(), 2);
        assert_eq!(selection_for(&merged, "model-a").unwrap().tier, Some(ReasoningTier::Max));
        assert_eq!(selection_for(&merged, "model-b").unwrap().tier, Some(ReasoningTier::Deep));
    }

    /// 模型消失后剪枝；仍在列表里的选择保留。
    #[test]
    fn pruning_drops_only_vanished_models() {
        let mut selections = vec![
            ReasoningSelection::new("model-a", ReasoningTier::Light, SelectionSource::User),
            ReasoningSelection::new("model-gone", ReasoningTier::Deep, SelectionSource::User),
        ];
        prune_missing(&mut selections, &[model("model-a")]);
        assert_eq!(selections.len(), 1);
        assert_eq!(selections[0].model_id, "model-a");
    }

    /// 每个模型一条选择：同 Provider 下的两个模型可以持不同档位。
    /// 这是"reasoning 不是全局配置"的核心断言。
    #[test]
    fn two_models_under_one_provider_hold_different_tiers() {
        let mut selections = Vec::new();
        upsert(&mut selections, ReasoningSelection::new("model-x", ReasoningTier::Deep, SelectionSource::User));
        upsert(
            &mut selections,
            ReasoningSelection::with_binding("model-y", ReasoningBinding::Budget { tokens: 8000 }, SelectionSource::User),
        );

        assert_eq!(selection_for(&selections, "model-x").unwrap().tier, Some(ReasoningTier::Deep));
        assert_eq!(
            selection_for(&selections, "model-y").unwrap().explicit_binding,
            Some(ReasoningBinding::Budget { tokens: 8000 })
        );
    }

    /// upsert 同一模型两次不产生重复条目。
    #[test]
    fn upsert_replaces_instead_of_duplicating() {
        let mut selections = Vec::new();
        upsert(&mut selections, ReasoningSelection::new("m", ReasoningTier::Light, SelectionSource::User));
        upsert(&mut selections, ReasoningSelection::new("m", ReasoningTier::Max, SelectionSource::User));
        assert_eq!(selections.len(), 1);
        assert_eq!(selections[0].tier, Some(ReasoningTier::Max));
    }

    /// 旧 state.json 里的 Provider 没有 reasoningSelections 字段，必须能反序列化成空列表。
    #[test]
    fn legacy_provider_json_deserializes_with_empty_selections() {
        let json = serde_json::json!({
            "id": "p1", "name": "旧配置", "baseUrl": "https://api.example.com/v1",
            "protocol": "openai", "enabled": true, "isCurrent": true, "defaultModel": "m",
            "models": [], "connectionState": "connected", "confidence": 0.9,
            "lastCheckedAt": null, "appliedClients": [], "errorSummary": null
        });
        let provider: crate::model::Provider = serde_json::from_value(json).expect("旧 Provider JSON 无法反序列化");
        assert!(provider.reasoning_selections.is_empty());
    }

    /// 选择的序列化契约：camelCase + tier 用小写字符串 + binding 内部标签。
    #[test]
    fn selection_serializes_as_camel_case_contract() {
        let selection = ReasoningSelection::new("m", ReasoningTier::Deep, SelectionSource::User);
        let json = serde_json::to_value(&selection).expect("序列化失败");
        assert_eq!(json["modelId"], "m");
        assert_eq!(json["tier"], "deep");
        assert_eq!(json["source"], "user");
        assert!(json["chosenAt"].is_string());

        let with_binding = ReasoningSelection::with_binding(
            "m",
            ReasoningBinding::Budget { tokens: 8000 },
            SelectionSource::User,
        );
        let json = serde_json::to_value(&with_binding).expect("序列化失败");
        assert_eq!(json["explicitBinding"]["kind"], "budget");
        assert_eq!(json["explicitBinding"]["tokens"], 8000);
    }
}

#[cfg(test)]
mod fallback_tests {
    use super::*;
    use crate::model::{
        AppSettings, CustomReasoningTier, NameMatchType, ProtocolKind, ReasoningFallback, ReasoningNameRule,
    };
    use crate::reasoning_capability::ReasoningKey;

    fn tier(id: &str, openai: Option<serde_json::Value>) -> CustomReasoningTier {
        CustomReasoningTier {
            id: id.to_owned(),
            label: format!("{id} 档"),
            description: None,
            openai_params: openai,
            anthropic_params: None,
            gemini_params: None,
        }
    }

    fn rule(pattern: &str, match_type: NameMatchType, tier_id: &str) -> ReasoningNameRule {
        ReasoningNameRule {
            id: format!("rule-{pattern}-{tier_id}"),
            pattern: pattern.to_owned(),
            match_type,
            tier_id: tier_id.to_owned(),
        }
    }

    /// 用例一：内置档位在 OpenAI 系协议下解析成 effort 参数。
    #[test]
    fn builtin_tiers_resolve_to_effort_on_openai() {
        let settings = AppSettings::default();
        for (id, effort) in [("light", "low"), ("standard", "medium"), ("deep", "high"), ("max", "high")] {
            let params = resolve_tier_config(id, &settings, ProtocolKind::Openai)
                .unwrap_or_else(|| panic!("内置档位 {id} 应当能解析出 OpenAI 参数"));
            assert_eq!(params["reasoning"]["effort"], effort, "档位 {id} 的 effort 取值不对");
        }
        // Azure 与 Custom 走同一套 OpenAI 兼容形状。
        for protocol in [ProtocolKind::AzureOpenai, ProtocolKind::Custom] {
            let params = resolve_tier_config("deep", &settings, protocol).expect("应当复用 OpenAI 形状");
            assert_eq!(params["reasoning"]["effort"], "high");
        }
    }

    /// 用例二：内置档位在 Anthropic / Gemini 下返回 None，因为程序不编造预算数字。
    #[test]
    fn builtin_tiers_have_no_params_for_budget_protocols() {
        let settings = AppSettings::default();
        for protocol in [ProtocolKind::Anthropic, ProtocolKind::Gemini] {
            assert!(
                resolve_tier_config("deep", &settings, protocol).is_none(),
                "{protocol:?}：内置档位不该凭空给出预算数字"
            );
        }
        // Off 档在任何协议下都没有可写表达：写 effort="off" 是编造取值。
        assert!(resolve_tier_config("off", &settings, ProtocolKind::Openai).is_none());
    }

    /// 用例三：自定义档位按协议取对应字段，参数原样返回不改写。
    #[test]
    fn custom_tiers_return_their_params_verbatim() {
        let anthropic = serde_json::json!({ "thinking": { "type": "enabled", "budget_tokens": 8192 } });
        let gemini = serde_json::json!({ "generationConfig": { "thinkingConfig": { "thinkingBudget": 4096 } } });
        let settings = AppSettings {
            custom_reasoning_tiers: vec![CustomReasoningTier {
                id: "tier-x".into(),
                label: "超深".into(),
                description: None,
                openai_params: Some(serde_json::json!({ "reasoning": { "effort": "xhigh" } })),
                anthropic_params: Some(anthropic.clone()),
                gemini_params: Some(gemini.clone()),
            }],
            ..AppSettings::default()
        };

        assert_eq!(
            resolve_tier_config("tier-x", &settings, ProtocolKind::Openai).expect("应当有 OpenAI 参数")["reasoning"]
                ["effort"],
            "xhigh"
        );
        assert_eq!(
            resolve_tier_config("tier-x", &settings, ProtocolKind::Anthropic).expect("应当有 Anthropic 参数"),
            anthropic
        );
        assert_eq!(
            resolve_tier_config("tier-x", &settings, ProtocolKind::Gemini).expect("应当有 Gemini 参数"),
            gemini
        );
    }

    /// 用例四：档位不存在、或该协议没填参数，都返回 None 而不是报错。
    #[test]
    fn missing_tier_or_missing_protocol_params_degrade_to_none() {
        let settings = AppSettings {
            custom_reasoning_tiers: vec![tier("only-openai", Some(serde_json::json!({ "reasoning": {} })))],
            ..AppSettings::default()
        };

        assert!(resolve_tier_config("never-created", &settings, ProtocolKind::Openai).is_none());
        assert!(resolve_tier_config("", &settings, ProtocolKind::Openai).is_none());
        assert!(resolve_tier_config("   ", &settings, ProtocolKind::Openai).is_none());
        // 档位存在，但没填 Anthropic 参数——这一级同样不可用。
        assert!(resolve_tier_config("only-openai", &settings, ProtocolKind::Anthropic).is_none());
        // id 是机器键，大小写不该被容忍，否则用户会以为两个写法是同一个档位。
        assert!(resolve_tier_config("Only-OpenAI", &settings, ProtocolKind::Openai).is_none());
    }

    /// 用例五：前缀与包含两种匹配，大小写不敏感，空 pattern 不参与。
    #[test]
    fn name_rules_match_prefix_and_contains_case_insensitively() {
        let rules = vec![
            rule("GLM-", NameMatchType::Prefix, "light"),
            rule("THINKING", NameMatchType::Contains, "deep"),
            rule("   ", NameMatchType::Contains, "max"),
        ];

        assert_eq!(match_name_fallback("glm-4-plus", &rules).map(|r| r.tier_id.as_str()), Some("light"));
        assert_eq!(match_name_fallback("Qwen-Thinking-Max", &rules).map(|r| r.tier_id.as_str()), Some("deep"));
        // 前缀规则不该被"名字中间含有该片段"命中。
        assert!(match_name_fallback("custom-glm-4", &rules).is_none());
        assert!(match_name_fallback("gpt-4o", &rules).is_none());
        // 空 pattern 那条如果参与匹配，上面这两条就会全部命中它。
        assert!(match_name_fallback("", &rules).is_none());
        assert!(match_name_fallback("   ", &rules).is_none());
    }

    /// 用例六：多条规则按数组顺序取首个命中，顺序即优先级。
    #[test]
    fn name_rule_order_is_priority() {
        let broad = rule("glm-", NameMatchType::Prefix, "light");
        let narrow = rule("glm-4", NameMatchType::Prefix, "deep");

        let broad_first = vec![broad.clone(), narrow.clone()];
        assert_eq!(match_name_fallback("glm-4-plus", &broad_first).map(|r| r.tier_id.as_str()), Some("light"));

        // 顺序颠倒，答案跟着变——顺序就是用户表达的优先级，程序不替他重排。
        let narrow_first = vec![narrow, broad];
        assert_eq!(match_name_fallback("glm-4-plus", &narrow_first).map(|r| r.tier_id.as_str()), Some("deep"));
    }

    /// 用例七：完整降级链。单模型兜底 > 名称规则 > None（交给全局档）。
    #[test]
    fn fallback_settlement_degrades_level_by_level() {
        let settings = AppSettings {
            reasoning_fallbacks: vec![ReasoningFallback {
                model_id: "glm-4-plus".into(),
                tier_id: "light".into(),
            }],
            reasoning_name_rules: vec![rule("glm-", NameMatchType::Prefix, "deep")],
            ..AppSettings::default()
        };

        // 单模型兜底压过名称规则：具体意图胜过宽泛意图。
        let hit = resolve_fallback_params("glm-4-plus", &settings, ProtocolKind::Openai).expect("应当命中单模型兜底");
        assert_eq!(hit.origin, FallbackOrigin::ModelFallback);
        assert_eq!(hit.params["reasoning"]["effort"], "low");
        assert_eq!(hit.origin.label(), "单模型兜底");

        // 同一份配置，另一个模型只命中规则。
        let hit = resolve_fallback_params("glm-3-turbo", &settings, ProtocolKind::Openai).expect("应当命中名称规则");
        assert_eq!(hit.origin, FallbackOrigin::NameRule);
        assert_eq!(hit.params["reasoning"]["effort"], "high");
        assert_eq!(hit.origin.label(), "名称规则兜底");

        // 两级都不命中：返回 None，由调用方走全局手动回退档。
        assert!(resolve_fallback_params("gpt-4o", &settings, ProtocolKind::Openai).is_none());
    }

    /// 单模型兜底指向已删除的档位时，降级到名称规则而不是直接掉到全局档。
    #[test]
    fn deleted_tier_in_model_fallback_degrades_to_name_rule() {
        let settings = AppSettings {
            reasoning_fallbacks: vec![ReasoningFallback {
                model_id: "glm-4-plus".into(),
                tier_id: "deleted-tier".into(),
            }],
            reasoning_name_rules: vec![rule("glm-", NameMatchType::Prefix, "deep")],
            ..AppSettings::default()
        };

        let hit = resolve_fallback_params("glm-4-plus", &settings, ProtocolKind::Openai).expect("应当降级到名称规则");
        assert_eq!(hit.origin, FallbackOrigin::NameRule);
        assert_eq!(hit.tier_id, "deep");
    }

    /// 首条命中的规则档位不可用时，继续试后面还能命中的规则。
    #[test]
    fn unusable_first_rule_falls_through_to_the_next_matching_rule() {
        let settings = AppSettings {
            reasoning_name_rules: vec![
                rule("glm-", NameMatchType::Prefix, "gone"),
                rule("glm-", NameMatchType::Prefix, "standard"),
            ],
            ..AppSettings::default()
        };

        let hit = resolve_fallback_params("glm-4-plus", &settings, ProtocolKind::Openai).expect("应当用第二条规则");
        assert_eq!(hit.tier_id, "standard");
        assert_eq!(hit.params["reasoning"]["effort"], "medium");

        // 所有能命中的规则都不可用时干净地返回 None，不死循环也不 panic。
        let dead = AppSettings {
            reasoning_name_rules: vec![
                rule("glm-", NameMatchType::Prefix, "gone"),
                rule("glm-", NameMatchType::Prefix, "also-gone"),
            ],
            ..AppSettings::default()
        };
        assert!(resolve_fallback_params("glm-4-plus", &dead, ProtocolKind::Openai).is_none());
    }

    /// 内置档位在 Anthropic 下不可用时，会继续降级到能给出参数的自定义档位。
    #[test]
    fn builtin_tier_unusable_on_anthropic_degrades_to_custom_tier() {
        let settings = AppSettings {
            custom_reasoning_tiers: vec![CustomReasoningTier {
                id: "budget-8k".into(),
                label: "8K 预算".into(),
                description: None,
                openai_params: None,
                anthropic_params: Some(serde_json::json!({ "thinking": { "budget_tokens": 8192 } })),
                gemini_params: None,
            }],
            reasoning_fallbacks: vec![ReasoningFallback {
                model_id: "claude-x".into(),
                // 内置档位在 Anthropic 下给不出参数，这一级会被跳过。
                tier_id: "deep".into(),
            }],
            reasoning_name_rules: vec![rule("claude-", NameMatchType::Prefix, "budget-8k")],
            ..AppSettings::default()
        };

        let hit = resolve_fallback_params("claude-x", &settings, ProtocolKind::Anthropic).expect("应当降级到自定义档位");
        assert_eq!(hit.origin, FallbackOrigin::NameRule);
        assert_eq!(hit.params["thinking"]["budget_tokens"], 8192);
    }

    /// 默认设置不含任何规则、档位、映射，兜底结算对任何模型都返回 None。
    #[test]
    fn default_settings_never_produce_a_fallback() {
        let settings = AppSettings::default();
        assert!(settings.reasoning_name_rules.is_empty());
        assert!(settings.custom_reasoning_tiers.is_empty());
        assert!(settings.reasoning_fallbacks.is_empty());
        for model in ["glm-4-plus", "gpt-5", "claude-opus-4", "gemini-2.5-pro", ""] {
            for protocol in [ProtocolKind::Openai, ProtocolKind::Anthropic, ProtocolKind::Gemini] {
                assert!(
                    resolve_fallback_params(model, &settings, protocol).is_none(),
                    "空配置下 {model} / {protocol:?} 不该产生兜底"
                );
            }
        }
    }

    /// 兜底结算不改变实时请求：未探明能力仍然 Omitted。
    ///
    /// 这条断言是本任务的核心边界。上面所有兜底配置都装进 settings，`resolve_binding`
    /// 压根不读它——它连 settings 参数都没有，所以未探明能力必然省略推理参数。
    #[test]
    fn live_requests_still_omit_reasoning_for_unexplored_models() {
        let unknown = ReasoningCapability::unknown(ReasoningKey::new("https://api.example.com/v1", "glm-4-plus"));
        let settings = AppSettings {
            reasoning_fallbacks: vec![ReasoningFallback {
                model_id: "glm-4-plus".into(),
                tier_id: "deep".into(),
            }],
            reasoning_name_rules: vec![rule("glm-", NameMatchType::Prefix, "max")],
            ..AppSettings::default()
        };

        // 配置写出这一侧确实有兜底。
        assert!(resolve_fallback_params("glm-4-plus", &settings, ProtocolKind::Openai).is_some());
        // 实时请求这一侧仍然什么都不发，避免网关 400。
        let resolved = resolve_binding(Some(&unknown), None, None);
        assert_eq!(resolved.binding, ReasoningBinding::Omitted);
        assert!(resolved.tier.is_none());
    }
}
