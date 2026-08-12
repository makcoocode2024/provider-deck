use serde_json::{json, Value};

use super::{ErrorInterpretation, IntrospectionTarget, MetadataHints, ReasoningAdapter, ValidationProbe};
use crate::reasoning_capability::{ReasoningCapability, ReasoningTier};

pub struct AnthropicAdapter;

impl ReasoningAdapter for AnthropicAdapter {
    fn metadata_hints(&self, model_json: &Value) -> MetadataHints {
        let mut hints = MetadataHints::default();

        // Anthropic 模型元数据可能包含 thinking 相关字段
        if let Some(capabilities) = model_json.get("capabilities") {
            if capabilities.get("thinking").is_some() || capabilities.get("extended_thinking").is_some() {
                hints.reasoning_fields.push("capabilities.thinking".into());
            }
        }

        // 如果元数据直接声明 thinking_budget_range，提取之
        if let Some(min) = model_json.pointer("/thinking/budget_min").and_then(Value::as_u64) {
            if let Some(max) = model_json.pointer("/thinking/budget_max").and_then(Value::as_u64) {
                hints.budget_range = Some((min, max));
                hints.reasoning_fields.push("thinking.budget_range".into());
            }
        }

        hints
    }

    fn metadata_target(&self, model_id: &str) -> Option<super::MetadataTarget> {
        Some(super::MetadataTarget {
            endpoint: format!("/v1/models/{model_id}"),
            auth: super::AuthScheme::AnthropicKey,
        })
    }

    fn introspection_targets(&self, _model_id: &str) -> Vec<IntrospectionTarget> {
        // Anthropic 目前没有免费的独立 capability 查询接口
        Vec::new()
    }

    fn validation_probe(&self, model_id: &str) -> Option<ValidationProbe> {
        // 发送一个故意超出合理范围的 budget_tokens，触发校验错误
        // Anthropic 典型约束：budget < max_tokens，且有上下限
        Some(ValidationProbe {
            endpoint: "/v1/messages".into(),
            body: json!({
                "model": model_id,
                "messages": [{"role": "user", "content": "Provider Deck capability validation."}],
                "thinking": {
                    "type": "enabled",
                    "budget_tokens": 999999
                },
                "stream": false
            }),
            auth: super::AuthScheme::AnthropicKey,
            output_limits: vec![super::OutputLimitPatch::at("/max_tokens")],
        })
    }

    fn interpret_error(&self, status: u16, body: &Value) -> ErrorInterpretation {
        if status != 400 && status != 422 {
            return ErrorInterpretation::Unknown;
        }

        let message = body
            .pointer("/error/message")
            .or_else(|| body.get("message"))
            .and_then(Value::as_str)
            .unwrap_or("");

        let lower = message.to_lowercase();

        // Anthropic 典型错误：
        // "thinking.budget_tokens must be less than max_tokens"
        // "thinking.budget_tokens must be between 1024 and 10000"
        // "Unrecognized field: thinking"
        // "thinking is not supported for this model"

        if lower.contains("must be between") || lower.contains("range") {
            // 尝试提取数值范围
            if let Some(range) = extract_budget_range(message) {
                return ErrorInterpretation::Supported {
                    detail: format!("参数校验确认预算范围：{} - {} tokens", range.0, range.1),
                };
            }
            return ErrorInterpretation::Supported {
                detail: format!("错误消息提示预算约束：{message}"),
            };
        }

        if lower.contains("less than max_tokens") || lower.contains("below max_tokens") {
            return ErrorInterpretation::Supported {
                detail: "确认约束：thinking.budget_tokens 必须小于 max_tokens".into(),
            };
        }

        if lower.contains("unrecognized") && lower.contains("thinking") {
            return ErrorInterpretation::Unsupported {
                detail: "服务端拒绝 thinking 参数（unrecognized field）".into(),
            };
        }

        if lower.contains("not supported") && lower.contains("thinking") {
            return ErrorInterpretation::Unsupported {
                detail: "服务端明确表示该模型不支持 thinking".into(),
            };
        }

        ErrorInterpretation::Unknown
    }

    fn apply_reasoning_config(&self, capability: &ReasoningCapability, tier: ReasoningTier) -> Option<Value> {
        use crate::reasoning_capability::ReasoningBinding;

        let option = capability.tier(tier)?;
        match &option.binding {
            ReasoningBinding::Budget { tokens } => Some(json!({
                "thinking": {
                    "type": "enabled",
                    "budget_tokens": tokens
                }
            })),
            ReasoningBinding::Enabled => Some(json!({
                "thinking": {
                    "type": "enabled"
                }
            })),
            ReasoningBinding::Disabled => Some(json!({
                "thinking": {
                    "type": "disabled"
                }
            })),
            ReasoningBinding::Omitted => None,
            _ => None, // Anthropic 不使用 Effort/DynamicBudget
        }
    }

    fn observe_usage(&self, response: &Value) -> (Option<u64>, Option<u64>) {
        // Anthropic 响应格式（streaming 时在 message_stop 事件）：
        // "usage": {
        //   "input_tokens": 10,
        //   "output_tokens": 20,
        //   "cache_creation_tokens": 0,
        //   "cache_read_tokens": 0
        // }
        // 未来可能新增 thinking_tokens 或类似字段，目前暂无

        // 检查是否有 thinking_tokens 或 reasoning_tokens
        let thinking = response
            .pointer("/usage/thinking_tokens")
            .or_else(|| response.pointer("/usage/reasoning_tokens"))
            .and_then(Value::as_u64);

        (None, thinking)
    }
}

/// 从错误消息中提取预算范围，例如 "must be between 1024 and 10000"
fn extract_budget_range(message: &str) -> Option<(u64, u64)> {
    let lower = message.to_lowercase();
    if let Some(start) = lower.find("between") {
        let rest = &message[start + 7..];
        let parts: Vec<&str> = rest.split_whitespace().collect();
        if parts.len() >= 3 {
            let min = parts[0].trim_matches(|c: char| !c.is_ascii_digit()).parse::<u64>().ok()?;
            let max = parts[2].trim_matches(|c: char| !c.is_ascii_digit()).parse::<u64>().ok()?;
            return Some((min, max));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interprets_budget_range_constraint() {
        let adapter = AnthropicAdapter;
        let body = json!({
            "error": {
                "message": "thinking.budget_tokens must be between 1024 and 10000"
            }
        });
        match adapter.interpret_error(400, &body) {
            ErrorInterpretation::Supported { detail } => {
                assert!(detail.contains("1024") && detail.contains("10000"));
            }
            _ => panic!("Expected Supported"),
        }
    }

    #[test]
    fn interprets_max_tokens_constraint() {
        let adapter = AnthropicAdapter;
        let body = json!({
            "error": {
                "message": "thinking.budget_tokens must be less than max_tokens"
            }
        });
        match adapter.interpret_error(400, &body) {
            ErrorInterpretation::Supported { detail } => {
                assert!(detail.contains("max_tokens"));
            }
            _ => panic!("Expected Supported"),
        }
    }

    #[test]
    fn interprets_unsupported_thinking() {
        let adapter = AnthropicAdapter;
        let body = json!({ "error": { "message": "thinking is not supported for this model" } });
        match adapter.interpret_error(400, &body) {
            ErrorInterpretation::Unsupported { .. } => {}
            _ => panic!("Expected Unsupported"),
        }
    }

    #[test]
    fn applies_budget_binding() {
        let adapter = AnthropicAdapter;
        let key = crate::reasoning_capability::ReasoningKey::new("https://api.anthropic.com/v1", "claude-sonnet-4");
        let capability = crate::reasoning_capability::ReasoningCapability::from_token_budget(
            key,
            1024,
            10000,
            true,
            None,
            crate::reasoning_capability::ReasoningConfidence::Validated,
        );
        let config = adapter.apply_reasoning_config(&capability, ReasoningTier::Deep).unwrap();
        assert_eq!(config["thinking"]["type"], "enabled");
        assert!(config["thinking"]["budget_tokens"].as_u64().unwrap() > 1024);
    }

    #[test]
    fn applies_disabled_binding() {
        let adapter = AnthropicAdapter;
        let key = crate::reasoning_capability::ReasoningKey::new("https://api.anthropic.com/v1", "claude-sonnet-4");
        let capability = crate::reasoning_capability::ReasoningCapability::from_token_budget(
            key,
            1024,
            10000,
            true,
            None,
            crate::reasoning_capability::ReasoningConfidence::Validated,
        );
        let config = adapter.apply_reasoning_config(&capability, ReasoningTier::Off).unwrap();
        assert_eq!(config["thinking"]["type"], "disabled");
    }

    #[test]
    fn extracts_budget_range_from_error_message() {
        assert_eq!(extract_budget_range("thinking.budget_tokens must be between 1024 and 10000"), Some((1024, 10000)));
        assert_eq!(extract_budget_range("value must be between 512 and 8192 tokens"), Some((512, 8192)));
    }

    /// 名字里带 claude-opus 也不能凭名字判定能力。
    #[test]
    fn model_name_alone_yields_no_hints() {
        let adapter = AnthropicAdapter;
        let hints = adapter.metadata_hints(&json!({ "id": "claude-opus-4-thinking", "type": "model" }));
        assert!(hints.reasoning_fields.is_empty());
        assert!(hints.budget_range.is_none());
    }

    #[test]
    fn reads_declared_budget_range() {
        let adapter = AnthropicAdapter;
        let hints = adapter.metadata_hints(&json!({
            "id": "anything",
            "thinking": { "budget_min": 1024, "budget_max": 32000 }
        }));
        assert_eq!(hints.budget_range, Some((1024, 32000)));
    }

    #[test]
    fn applies_enabled_binding_for_toggle_only_capability() {
        let adapter = AnthropicAdapter;
        let key = crate::reasoning_capability::ReasoningKey::new("https://api.anthropic.com/v1", "m");
        let capability = crate::reasoning_capability::ReasoningCapability::from_boolean_toggle(
            key,
            true,
            crate::reasoning_capability::ReasoningConfidence::Validated,
        );
        let config = adapter.apply_reasoning_config(&capability, ReasoningTier::Standard).unwrap();
        assert_eq!(config["thinking"]["type"], "enabled");
        assert!(config["thinking"].get("budget_tokens").is_none());
    }

    /// unknown 能力没有任何档位，映射必须返回 None（Step 5 据此省略参数）。
    #[test]
    fn unknown_capability_maps_to_nothing() {
        let adapter = AnthropicAdapter;
        let capability = crate::reasoning_capability::ReasoningCapability::unknown(
            crate::reasoning_capability::ReasoningKey::new("https://api.anthropic.com/v1", "m"),
        );
        assert!(adapter.apply_reasoning_config(&capability, ReasoningTier::Standard).is_none());
        assert!(adapter.apply_reasoning_config(&capability, ReasoningTier::Off).is_none());
    }

    #[test]
    fn probe_sends_out_of_range_budget() {
        let adapter = AnthropicAdapter;
        let probe = adapter.validation_probe("some-model").expect("probe missing");
        assert_eq!(probe.endpoint, "/v1/messages");
        assert_eq!(probe.body["thinking"]["budget_tokens"], 999_999);
        assert_eq!(probe.output_limits, vec![super::super::OutputLimitPatch::at("/max_tokens")]);
    }
}
