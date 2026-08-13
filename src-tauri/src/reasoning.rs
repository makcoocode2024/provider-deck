use crate::{activity, model::AppSettings};

/// 结算「能力未探明时的回退档位」。
///
/// 这里**没有**推荐逻辑：生效档位恒等于用户显式选择的 `manual_reasoning_level`。
/// 曾经存在的按模型名 / 参数量 / 本机显存打分的自动推荐已删除——推理能力归属
/// `(base_url, model_id)`，由 `reasoning_discovery` 探测得出，不能靠模型名猜。
///
/// `effective_reasoning_level` 本身保留：它是 [`crate::reasoning_selection::resolve_binding`]
/// 的 legacy fallback，也是 `config.rs` 在能力未探明时不抹掉既有
/// `model_reasoning_effort` 的依据。
/// 清理用户提交的兜底表。
///
/// 三件事，都在写入边界做完，让读路径（`AppSettings::reasoning_fallback_for`）
/// 可以只做全等比较：
/// - trim 掉 model_id 两端空白：从模型列表复制粘贴常带空格，留着会让全等匹配永不命中
/// - 丢掉空 model_id：那是"还没填"的行，不是一条设定
/// - 同一 model_id 只留**最后**一条：UI 允许重复输入，后写的是用户的最新意图
///
/// 注意：**不校验 tier_id 是否存在**。档位可能是内置的，也可能是自定义的，
/// 还可能是用户刚删掉的——后者要走结算时的平滑降级，而不是在保存时被抹掉。
/// 抹掉等于用户重建档位后兜底不会自动恢复。
fn sanitize_fallbacks(settings: &mut AppSettings) {
    let mut cleaned: Vec<crate::model::ReasoningFallback> = Vec::new();
    for item in settings.reasoning_fallbacks.drain(..) {
        let model_id = item.model_id.trim().to_owned();
        if model_id.is_empty() { continue; }
        let entry = crate::model::ReasoningFallback { model_id, tier_id: item.tier_id.trim().to_owned() };
        match cleaned.iter_mut().find(|existing| existing.model_id == entry.model_id) {
            Some(existing) => existing.tier_id = entry.tier_id,
            None => cleaned.push(entry),
        }
    }
    settings.reasoning_fallbacks = cleaned;
}

/// 清理模型名规则表。和兜底表同理，只做写入边界的机械清理：
/// - trim pattern，丢掉空 pattern 的行（空 pattern 会命中一切模型）
/// - **保留顺序、保留重复**：模块二的匹配按数组顺序取首个命中，顺序本身是用户意图
fn sanitize_name_rules(settings: &mut AppSettings) {
    settings.reasoning_name_rules.retain_mut(|rule| {
        rule.pattern = rule.pattern.trim().to_owned();
        rule.tier_id = rule.tier_id.trim().to_owned();
        !rule.pattern.is_empty() && !rule.tier_id.is_empty()
    });
}

/// 清理自定义档位表：trim 掉 id 和名称，丢掉没有 id 的行。
/// 三个协议参数一律不动——那是用户手写的 JSON，程序无权改写。
fn sanitize_custom_tiers(settings: &mut AppSettings) {
    settings.custom_reasoning_tiers.retain_mut(|tier| {
        tier.id = tier.id.trim().to_owned();
        tier.label = tier.label.trim().to_owned();
        !tier.id.is_empty()
    });
}

pub fn refresh_settings(settings: &mut AppSettings, log_action: bool) {
    settings.effective_reasoning_level = settings.manual_reasoning_level;
    sanitize_fallbacks(settings);
    sanitize_custom_tiers(settings);
    sanitize_name_rules(settings);
    if log_action {
        activity::record(
            "manual_reasoning",
            &format!("切换为{}档", settings.manual_reasoning_level.as_str()),
            true,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ReasoningLevel;

    #[test]
    fn effective_level_follows_manual_choice() {
        let mut settings = AppSettings {
            manual_reasoning_level: ReasoningLevel::Low,
            effective_reasoning_level: ReasoningLevel::High,
            ..AppSettings::default()
        };

        refresh_settings(&mut settings, false);

        assert_eq!(settings.effective_reasoning_level, ReasoningLevel::Low);
    }

    fn fallback(model_id: &str, tier_id: &str) -> crate::model::ReasoningFallback {
        crate::model::ReasoningFallback { model_id: model_id.into(), tier_id: tier_id.into() }
    }

    /// 从模型列表复制模型名常带空白。不 trim 会让这条设定永远匹配不上，
    /// 而用户在界面上明明看到它存在。
    #[test]
    fn fallback_model_ids_are_trimmed_on_save() {
        let mut settings = AppSettings {
            reasoning_fallbacks: vec![fallback("  coder  ", " light ")],
            ..AppSettings::default()
        };

        refresh_settings(&mut settings, false);

        assert_eq!(settings.reasoning_fallbacks[0].model_id, "coder");
        assert_eq!(settings.reasoning_fallback_for("coder"), Some("light"));
    }

    /// 空行不是一条设定。空 model_id 留在表里会让读路径出现一条永不命中的死记录。
    #[test]
    fn blank_fallback_rows_are_dropped() {
        let mut settings = AppSettings {
            reasoning_fallbacks: vec![fallback("   ", "light"), fallback("", "deep"), fallback("coder", "standard")],
            ..AppSettings::default()
        };

        refresh_settings(&mut settings, false);

        assert_eq!(settings.reasoning_fallbacks.len(), 1);
        assert_eq!(settings.reasoning_fallbacks[0].model_id, "coder");
    }

    /// 同一模型重复时保留最后一条：后写的是用户的最新意图。
    /// 读路径取第一条，所以去重必须在写入边界完成。
    #[test]
    fn duplicate_fallbacks_keep_the_last_choice() {
        let mut settings = AppSettings {
            reasoning_fallbacks: vec![
                fallback("coder", "light"),
                fallback("writer", "deep"),
                fallback("coder", "standard"),
            ],
            ..AppSettings::default()
        };

        refresh_settings(&mut settings, false);

        assert_eq!(settings.reasoning_fallbacks.len(), 2);
        assert_eq!(settings.reasoning_fallback_for("coder"), Some("standard"));
        assert_eq!(settings.reasoning_fallback_for("writer"), Some("deep"));
    }

    /// 指向已删除档位的兜底记录**不许**在保存时被清掉。用户可能只是先删档位、
    /// 稍后重建；抹掉记录会让重建后兜底不自动恢复。失效由结算时降级处理。
    #[test]
    fn fallbacks_pointing_at_missing_tiers_survive_save() {
        let mut settings = AppSettings {
            reasoning_fallbacks: vec![fallback("coder", "deleted-tier")],
            ..AppSettings::default()
        };

        refresh_settings(&mut settings, false);

        assert_eq!(settings.reasoning_fallback_for("coder"), Some("deleted-tier"));
    }

    /// 清理不得动全局档位，也不得凭空造出兜底记录。
    #[test]
    fn sanitizing_leaves_other_settings_alone() {
        let mut settings = AppSettings { manual_reasoning_level: ReasoningLevel::Medium, ..AppSettings::default() };

        refresh_settings(&mut settings, false);

        assert!(settings.reasoning_fallbacks.is_empty());
        assert!(settings.custom_reasoning_tiers.is_empty());
        assert!(settings.reasoning_name_rules.is_empty());
        assert_eq!(settings.effective_reasoning_level, ReasoningLevel::Medium);
    }

    fn rule(pattern: &str, tier_id: &str) -> crate::model::ReasoningNameRule {
        crate::model::ReasoningNameRule {
            id: format!("r-{pattern}"),
            pattern: pattern.into(),
            match_type: crate::model::NameMatchType::Prefix,
            tier_id: tier_id.into(),
        }
    }

    /// 空 pattern 会命中一切模型，是最危险的一行；空 tier_id 指向不存在的档位。
    /// 两者都在写入边界丢掉。
    #[test]
    fn blank_name_rules_are_dropped() {
        let mut settings = AppSettings {
            reasoning_name_rules: vec![rule("  ", "light"), rule("glm-", "  "), rule(" glm-4 ", " light ")],
            ..AppSettings::default()
        };

        refresh_settings(&mut settings, false);

        assert_eq!(settings.reasoning_name_rules.len(), 1);
        assert_eq!(settings.reasoning_name_rules[0].pattern, "glm-4");
        assert_eq!(settings.reasoning_name_rules[0].tier_id, "light");
    }

    /// 规则顺序就是优先级，重复的 pattern 也不去重——匹配时首个命中生效，
    /// 这是用户可以在 UI 里排序控制的语义。
    #[test]
    fn name_rule_order_and_duplicates_are_preserved() {
        let mut settings = AppSettings {
            reasoning_name_rules: vec![rule("glm-", "light"), rule("glm-", "deep"), rule("qwen-", "standard")],
            ..AppSettings::default()
        };

        refresh_settings(&mut settings, false);

        let tiers: Vec<&str> = settings.reasoning_name_rules.iter().map(|r| r.tier_id.as_str()).collect();
        assert_eq!(tiers, vec!["light", "deep", "standard"]);
    }

    /// 自定义档位的协议参数是用户手写的 JSON，清理时一个字节都不许改。
    #[test]
    fn custom_tier_params_are_never_rewritten() {
        let params = serde_json::json!({"reasoning": {"effort": "xhigh"}});
        let mut settings = AppSettings {
            custom_reasoning_tiers: vec![crate::model::CustomReasoningTier {
                id: "  t1  ".into(),
                label: "  超深  ".into(),
                description: None,
                openai_params: Some(params.clone()),
                anthropic_params: None,
                gemini_params: None,
            }],
            ..AppSettings::default()
        };

        refresh_settings(&mut settings, false);

        assert_eq!(settings.custom_reasoning_tiers[0].id, "t1");
        assert_eq!(settings.custom_reasoning_tiers[0].label, "超深");
        assert_eq!(settings.custom_reasoning_tiers[0].openai_params, Some(params));
    }

    /// 没有 id 的自定义档位无法被任何规则引用，是一条死记录。
    #[test]
    fn custom_tiers_without_id_are_dropped() {
        let mut settings = AppSettings {
            custom_reasoning_tiers: vec![crate::model::CustomReasoningTier {
                id: "   ".into(),
                label: "无名".into(),
                description: None,
                openai_params: Some(serde_json::json!({})),
                anthropic_params: None,
                gemini_params: None,
            }],
            ..AppSettings::default()
        };

        refresh_settings(&mut settings, false);

        assert!(settings.custom_reasoning_tiers.is_empty());
    }
}
