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
pub fn refresh_settings(settings: &mut AppSettings, log_action: bool) {
    settings.effective_reasoning_level = settings.manual_reasoning_level;
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
}
