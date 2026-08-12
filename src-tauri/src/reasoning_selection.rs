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
