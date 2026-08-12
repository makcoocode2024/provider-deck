use serde_json::{json, Value};

use super::{ErrorInterpretation, IntrospectionTarget, MetadataHints, ReasoningAdapter, ValidationProbe};
use crate::reasoning_capability::{ReasoningCapability, ReasoningTier};

pub struct OpenAIAdapter;

impl ReasoningAdapter for OpenAIAdapter {
    fn metadata_hints(&self, model_json: &Value) -> MetadataHints {
        let mut hints = MetadataHints::default();

        // OpenAI 模型元数据通常不直接暴露 reasoning.effort 的取值范围
        // 但如果未来 API 添加了 capabilities 字段，可以在这里提取
        if let Some(capabilities) = model_json.get("capabilities") {
            if capabilities.get("reasoning").is_some() {
                hints.reasoning_fields.push("capabilities.reasoning".into());
            }
        }

        hints
    }

    fn metadata_target(&self, model_id: &str) -> Option<super::MetadataTarget> {
        Some(super::MetadataTarget {
            endpoint: format!("/v1/models/{model_id}"),
            auth: super::AuthScheme::Bearer,
        })
    }

    fn introspection_targets(&self, _model_id: &str) -> Vec<IntrospectionTarget> {
        // OpenAI 目前没有免费的 reasoning capability introspection 端点
        Vec::new()
    }

    fn validation_probe(&self, model_id: &str) -> Option<ValidationProbe> {
        // 发送一个故意无效的 reasoning.effort 值，触发枚举校验错误
        Some(ValidationProbe {
            endpoint: "/v1/chat/completions".into(),
            body: json!({
                "model": model_id,
                "messages": [{"role": "user", "content": "Provider Deck capability validation."}],
                "reasoning": {
                    "effort": "invalid_effort_value_for_validation"
                },
                "stream": false
            }),
            auth: super::AuthScheme::Bearer,
            // 两个候选都写：o 系列只认 max_completion_tokens 并拒绝 max_tokens，
            // 而大量 OpenAI 兼容网关只认 max_tokens。多写一个字段不增加成本。
            output_limits: vec![
                super::OutputLimitPatch::at("/max_tokens"),
                super::OutputLimitPatch::at("/max_completion_tokens"),
            ],
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

        // OpenAI 典型错误：
        // "Invalid value for 'reasoning.effort'. Supported values are: low, medium, high"
        // "Unrecognized request argument supplied: reasoning"

        if lower.contains("supported values are") || lower.contains("allowed values are") {
            // 提取枚举成员
            if let Some(start) = message.rfind(':') {
                let values_part = &message[start + 1..];
                let members: Vec<String> = values_part
                    .split(',')
                    .map(|s| s.trim().trim_matches(|c| c == '\'' || c == '"' || c == '.').to_owned())
                    .filter(|s| !s.is_empty())
                    .collect();
                if !members.is_empty() {
                    return ErrorInterpretation::Supported {
                        detail: format!("参数校验枚举了 {} 个有效值：{}", members.len(), members.join(", ")),
                    };
                }
            }
            return ErrorInterpretation::Supported {
                detail: format!("错误消息提示支持的取值：{message}"),
            };
        }

        if lower.contains("unrecognized") && lower.contains("reasoning") {
            return ErrorInterpretation::Unsupported {
                detail: "服务端拒绝 reasoning 参数（unrecognized）".into(),
            };
        }

        if lower.contains("not supported") && lower.contains("reasoning") {
            return ErrorInterpretation::Unsupported {
                detail: "服务端明确表示不支持 reasoning".into(),
            };
        }

        ErrorInterpretation::Unknown
    }

    fn apply_reasoning_config(&self, capability: &ReasoningCapability, tier: ReasoningTier) -> Option<Value> {
        use crate::reasoning_capability::ReasoningBinding;

        let option = capability.tier(tier)?;
        match &option.binding {
            ReasoningBinding::Effort { value } => Some(json!({ "reasoning": { "effort": value } })),
            ReasoningBinding::Disabled => Some(json!({ "reasoning": { "effort": "off" } })),
            ReasoningBinding::Omitted => None,
            _ => None, // OpenAI 不使用 Budget/DynamicBudget/Enabled
        }
    }

    fn observe_usage(&self, response: &Value) -> (Option<u64>, Option<u64>) {
        // OpenAI 响应格式：
        // "usage": {
        //   "prompt_tokens": 10,
        //   "completion_tokens": 20,
        //   "completion_tokens_details": {
        //     "reasoning_tokens": 15
        //   }
        // }
        let reasoning_tokens = response
            .pointer("/usage/completion_tokens_details/reasoning_tokens")
            .and_then(Value::as_u64);

        // 输入推理 token OpenAI 目前不单独计费，全部算在 prompt_tokens 里
        (None, reasoning_tokens)
    }

    fn has_reasoning_in_response(&self, response: &Value) -> bool {
        // OpenAI 推理响应标识：
        // 1. usage.completion_tokens_details.reasoning_tokens > 0
        let has_reasoning_tokens = response
            .pointer("/usage/completion_tokens_details/reasoning_tokens")
            .and_then(Value::as_u64)
            .map(|n| n > 0)
            .unwrap_or(false);

        // 2. choices[0].message.reasoning 存在（某些 o1 系列返回推理摘要）
        let has_reasoning_field = response
            .pointer("/choices/0/message/reasoning")
            .is_some();

        has_reasoning_tokens || has_reasoning_field
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interprets_supported_values_enumeration() {
        let adapter = OpenAIAdapter;
        let body = json!({
            "error": {
                "message": "Invalid value for 'reasoning.effort'. Supported values are: low, medium, high, max"
            }
        });
        match adapter.interpret_error(400, &body) {
            ErrorInterpretation::Supported { detail } => {
                assert!(detail.contains("4 个有效值") || detail.contains("low"));
            }
            _ => panic!("Expected Supported"),
        }
    }

    #[test]
    fn interprets_unrecognized_as_unsupported() {
        let adapter = OpenAIAdapter;
        let body = json!({ "error": { "message": "Unrecognized request argument supplied: reasoning" } });
        match adapter.interpret_error(400, &body) {
            ErrorInterpretation::Unsupported { .. } => {}
            _ => panic!("Expected Unsupported"),
        }
    }

    #[test]
    fn applies_effort_binding() {
        let adapter = OpenAIAdapter;
        let key = crate::reasoning_capability::ReasoningKey::new("https://api.openai.com/v1", "o1");
        let capability = crate::reasoning_capability::ReasoningCapability::from_effort_enum(
            key,
            &["low".into(), "medium".into(), "high".into()],
            crate::reasoning_capability::ReasoningConfidence::Validated,
        );
        let config = adapter.apply_reasoning_config(&capability, ReasoningTier::Standard).unwrap();
        assert_eq!(config["reasoning"]["effort"], "medium");
    }

    #[test]
    fn extracts_reasoning_tokens_from_usage() {
        let adapter = OpenAIAdapter;
        let response = json!({
            "usage": {
                "prompt_tokens": 10,
                "completion_tokens": 25,
                "completion_tokens_details": {
                    "reasoning_tokens": 15
                }
            }
        });
        let (input, output) = adapter.observe_usage(&response);
        assert_eq!(input, None);
        assert_eq!(output, Some(15));
    }

    /// 扩展词表（xhigh/max）必须原样保留，不能被裁剪成 low/medium/high。
    #[test]
    fn keeps_extended_effort_values() {
        let adapter = OpenAIAdapter;
        let body = json!({
            "error": { "message": "Invalid value: 'x'. Supported values are: low, medium, high, xhigh" }
        });
        let ErrorInterpretation::Supported { detail } = adapter.interpret_error(400, &body) else {
            panic!("Expected Supported");
        };
        assert!(detail.contains("xhigh"));
        assert!(detail.contains("4 个有效值"));
    }

    /// 名字里带 o1 / reason 也不能凭名字判定能力。
    #[test]
    fn model_name_alone_yields_no_hints() {
        let adapter = OpenAIAdapter;
        let hints = adapter.metadata_hints(&json!({ "id": "o1-preview-reasoner", "owned_by": "openai" }));
        assert!(hints.reasoning_fields.is_empty());
        assert!(hints.effort_values.is_empty());
    }

    #[test]
    fn non_validation_status_is_unknown() {
        let adapter = OpenAIAdapter;
        let body = json!({ "error": { "message": "Supported values are: low, high" } });
        assert_eq!(adapter.interpret_error(500, &body), ErrorInterpretation::Unknown);
        assert_eq!(adapter.interpret_error(401, &body), ErrorInterpretation::Unknown);
    }

    #[test]
    fn probe_sends_out_of_range_effort() {
        let adapter = OpenAIAdapter;
        let probe = adapter.validation_probe("some-model").expect("probe missing");
        assert_eq!(probe.endpoint, "/v1/chat/completions");
        assert_eq!(probe.body["model"], "some-model");
        assert!(probe.body["reasoning"]["effort"].as_str().unwrap().contains("invalid"));
    }

    /// o 系列拒绝 max_tokens、兼容网关只认 max_tokens，两个候选都要声明。
    #[test]
    fn declares_both_output_limit_candidates() {
        let probe = OpenAIAdapter.validation_probe("m").expect("probe missing");
        let pointers: Vec<&str> = probe.output_limits.iter().map(|patch| patch.pointer.as_str()).collect();
        assert!(pointers.contains(&"/max_tokens"));
        assert!(pointers.contains(&"/max_completion_tokens"));
    }

    #[test]
    fn detects_reasoning_tokens_in_response() {
        let adapter = OpenAIAdapter;
        let response = json!({
            "choices": [{"message": {"content": "OK"}}],
            "usage": {
                "prompt_tokens": 10,
                "completion_tokens": 20,
                "completion_tokens_details": {
                    "reasoning_tokens": 15
                }
            }
        });
        assert!(adapter.has_reasoning_in_response(&response));
    }

    #[test]
    fn detects_reasoning_field_in_response() {
        let adapter = OpenAIAdapter;
        let response = json!({
            "choices": [{"message": {"content": "OK", "reasoning": "思考过程摘要"}}],
            "usage": {
                "prompt_tokens": 10,
                "completion_tokens": 20
            }
        });
        assert!(adapter.has_reasoning_in_response(&response));
    }

    #[test]
    fn rejects_response_without_reasoning() {
        let adapter = OpenAIAdapter;
        let response = json!({
            "choices": [{"message": {"content": "OK"}}],
            "usage": {
                "prompt_tokens": 10,
                "completion_tokens": 20
            }
        });
        assert!(!adapter.has_reasoning_in_response(&response));
    }

    #[test]
    fn rejects_response_with_zero_reasoning_tokens() {
        let adapter = OpenAIAdapter;
        let response = json!({
            "choices": [{"message": {"content": "OK"}}],
            "usage": {
                "prompt_tokens": 10,
                "completion_tokens": 20,
                "completion_tokens_details": {
                    "reasoning_tokens": 0
                }
            }
        });
        assert!(!adapter.has_reasoning_in_response(&response));
    }
}
