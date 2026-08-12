use serde_json::{json, Value};

use super::{ErrorInterpretation, IntrospectionTarget, MetadataHints, ReasoningAdapter, ValidationProbe};
use crate::reasoning_capability::{ReasoningCapability, ReasoningTier};

pub struct GeminiAdapter;

impl ReasoningAdapter for GeminiAdapter {
    fn metadata_hints(&self, model_json: &Value) -> MetadataHints {
        let mut hints = MetadataHints::default();

        // Gemini 模型元数据可能在 supportedGenerationMethods 或单独的 capabilities 中声明 thinking
        if let Some(methods) = model_json.get("supportedGenerationMethods").and_then(Value::as_array) {
            if methods.iter().any(|v| v.as_str().map_or(false, |s| s.contains("thinking") || s.contains("Thinking"))) {
                hints.reasoning_fields.push("supportedGenerationMethods.thinking".into());
            }
        }

        // 元数据直接标注 thinkingConfig 时视为服务端声明
        if model_json.get("thinkingConfig").is_some() {
            hints.reasoning_fields.push("thinkingConfig".into());
        }

        // 服务端声明的预算区间，字段名取 Gemini 常见的几种写法
        let min = model_json.pointer("/thinkingConfig/thinkingBudgetMin").and_then(Value::as_u64);
        let max = model_json
            .pointer("/thinkingConfig/thinkingBudgetMax")
            .or_else(|| model_json.pointer("/thinkingConfig/maxThinkingBudget"))
            .and_then(Value::as_u64);
        if let Some(max) = max {
            hints.budget_range = Some((min.unwrap_or(0), max));
            hints.reasoning_fields.push("thinkingConfig.thinkingBudget 区间".into());
        }

        // cannot_disable 只接受服务端显式声明，绝不由模型名推断；
        // 未声明时留给 Tier 2 validation probe 去确认。
        if let Some(false) = model_json
            .pointer("/thinkingConfig/canDisableThinking")
            .and_then(Value::as_bool)
        {
            hints.cannot_disable = true;
        }

        // -1 是 thinkingBudget 的协议级哨兵值（交由模型自行分配），属于协议常量而非模型清单
        hints.dynamic_sentinel = Some(-1);

        hints
    }

    fn metadata_target(&self, model_id: &str) -> Option<super::MetadataTarget> {
        Some(super::MetadataTarget {
            endpoint: format!("/v1beta/models/{}", model_id.strip_prefix("models/").unwrap_or(model_id)),
            auth: super::AuthScheme::GoogleKey,
        })
    }

    fn introspection_targets(&self, model_id: &str) -> Vec<IntrospectionTarget> {
        // Gemini v1beta 的 /models/{id} 端点可能返回能力细节
        vec![IntrospectionTarget {
            endpoint: format!("/v1beta/models/{}", model_id.strip_prefix("models/").unwrap_or(model_id)),
            method: "GET",
            auth: super::AuthScheme::GoogleKey,
            extract_path: vec!["thinkingConfig".into(), "supportedGenerationMethods".into()],
        }]
    }

    fn validation_probe(&self, model_id: &str) -> Option<ValidationProbe> {
        // 发送一个故意无效的 thinkingConfig.thinkingBudget 值，触发校验错误
        Some(ValidationProbe {
            endpoint: "/v1beta/models/{}:generateContent".replace("{}", model_id.strip_prefix("models/").unwrap_or(model_id)),
            body: json!({
                "contents": [{"role": "user", "parts": [{"text": "Provider Deck capability validation."}]}],
                "generationConfig": {
                    "thinkingConfig": {
                        "thinkingBudget": 999999
                    }
                }
            }),
            auth: super::AuthScheme::GoogleKey,
            output_limits: vec![super::OutputLimitPatch::at("/generationConfig/maxOutputTokens")],
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

        // Gemini 典型错误：
        // "thinkingBudget must be between 0 and 8192"
        // "thinkingConfig is not supported for this model"
        // "Invalid value for thinkingBudget"
        // "This model does not support disabling thinking"

        if lower.contains("must be between") || lower.contains("valid range") {
            // 提取数值范围
            if let Some(range) = extract_thinking_budget_range(message) {
                return ErrorInterpretation::Supported {
                    detail: format!("参数校验确认 thinkingBudget 范围：{} - {} tokens", range.0, range.1),
                };
            }
            return ErrorInterpretation::Supported {
                detail: format!("错误消息提示预算约束：{message}"),
            };
        }

        if lower.contains("thinkingconfig") && (lower.contains("not supported") || lower.contains("not available")) {
            return ErrorInterpretation::Unsupported {
                detail: "服务端明确表示该模型不支持 thinkingConfig".into(),
            };
        }

        if lower.contains("does not support disabling") || lower.contains("cannot disable thinking") {
            return ErrorInterpretation::Supported {
                detail: "该模型支持 thinking 但无法关闭（cannot_disable 约束）".into(),
            };
        }

        if lower.contains("invalid") && lower.contains("thinkingbudget") {
            // 可能是范围校验失败，但未明确枚举范围
            return ErrorInterpretation::Supported {
                detail: format!("参数校验确认 thinkingBudget 存在但取值无效：{message}"),
            };
        }

        ErrorInterpretation::Unknown
    }

    fn apply_reasoning_config(&self, capability: &ReasoningCapability, tier: ReasoningTier) -> Option<Value> {
        use crate::reasoning_capability::ReasoningBinding;

        let option = capability.tier(tier)?;
        match &option.binding {
            ReasoningBinding::Budget { tokens } => Some(json!({
                "generationConfig": {
                    "thinkingConfig": {
                        "thinkingBudget": tokens
                    }
                }
            })),
            ReasoningBinding::DynamicBudget { sentinel } => Some(json!({
                "generationConfig": {
                    "thinkingConfig": {
                        "thinkingBudget": sentinel
                    }
                }
            })),
            ReasoningBinding::Disabled => Some(json!({
                "generationConfig": {
                    "thinkingConfig": {
                        "thinkingBudget": 0
                    }
                }
            })),
            ReasoningBinding::Omitted => None,
            _ => None, // Gemini 不使用 Effort/Enabled
        }
    }

    fn observe_usage(&self, response: &Value) -> (Option<u64>, Option<u64>) {
        // Gemini 响应格式：
        // "usageMetadata": {
        //   "promptTokenCount": 10,
        //   "candidatesTokenCount": 20,
        //   "totalTokenCount": 30,
        //   "thinkingTokenCount": 15  // 可能的字段
        // }

        let thinking = response
            .pointer("/usageMetadata/thinkingTokenCount")
            .or_else(|| response.pointer("/usageMetadata/reasoningTokenCount"))
            .and_then(Value::as_u64);

        (None, thinking)
    }

    fn has_reasoning_in_response(&self, response: &Value) -> bool {
        // Gemini 推理响应标识：usageMetadata.thinkingTokenCount > 0
        response
            .pointer("/usageMetadata/thinkingTokenCount")
            .and_then(Value::as_u64)
            .map(|n| n > 0)
            .unwrap_or(false)
    }
}

/// 从 Gemini 错误消息中提取 thinkingBudget 范围，例如 "must be between 0 and 8192"
fn extract_thinking_budget_range(message: &str) -> Option<(u64, u64)> {
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
    fn interprets_thinking_budget_range() {
        let adapter = GeminiAdapter;
        let body = json!({
            "error": {
                "message": "thinkingBudget must be between 0 and 8192"
            }
        });
        match adapter.interpret_error(400, &body) {
            ErrorInterpretation::Supported { detail } => {
                assert!(detail.contains("0") && detail.contains("8192"));
            }
            _ => panic!("Expected Supported"),
        }
    }

    #[test]
    fn interprets_cannot_disable_as_supported_with_constraint() {
        let adapter = GeminiAdapter;
        let body = json!({
            "error": {
                "message": "This model does not support disabling thinking"
            }
        });
        match adapter.interpret_error(400, &body) {
            ErrorInterpretation::Supported { detail } => {
                assert!(detail.contains("cannot_disable"));
            }
            _ => panic!("Expected Supported"),
        }
    }

    #[test]
    fn interprets_unsupported_thinkingconfig() {
        let adapter = GeminiAdapter;
        let body = json!({ "error": { "message": "thinkingConfig is not supported for this model" } });
        match adapter.interpret_error(400, &body) {
            ErrorInterpretation::Unsupported { .. } => {}
            _ => panic!("Expected Unsupported"),
        }
    }

    #[test]
    fn applies_budget_binding() {
        let adapter = GeminiAdapter;
        let key = crate::reasoning_capability::ReasoningKey::new("https://generativelanguage.googleapis.com/v1beta", "gemini-2.0-flash-thinking-exp");
        let capability = crate::reasoning_capability::ReasoningCapability::from_token_budget(
            key,
            0,
            8192,
            true,
            Some(-1),
            crate::reasoning_capability::ReasoningConfidence::Validated,
        );
        let config = adapter.apply_reasoning_config(&capability, ReasoningTier::Deep).unwrap();
        assert!(config["generationConfig"]["thinkingConfig"]["thinkingBudget"].as_u64().is_some());
    }

    #[test]
    fn applies_dynamic_sentinel() {
        let adapter = GeminiAdapter;
        let key = crate::reasoning_capability::ReasoningKey::new("https://generativelanguage.googleapis.com/v1beta", "gemini-2.0-flash-thinking-exp");
        let capability = crate::reasoning_capability::ReasoningCapability::from_token_budget(
            key,
            0,
            8192,
            true,
            Some(-1),
            crate::reasoning_capability::ReasoningConfidence::Validated,
        );
        let config = adapter.apply_reasoning_config(&capability, ReasoningTier::Standard).unwrap();
        assert_eq!(config["generationConfig"]["thinkingConfig"]["thinkingBudget"], -1);
    }

    #[test]
    fn applies_disabled_as_zero_budget() {
        let adapter = GeminiAdapter;
        let key = crate::reasoning_capability::ReasoningKey::new("https://generativelanguage.googleapis.com/v1beta", "gemini-2.0-flash-thinking-exp");
        let capability = crate::reasoning_capability::ReasoningCapability::from_token_budget(
            key,
            0,
            8192,
            true,
            None,
            crate::reasoning_capability::ReasoningConfidence::Validated,
        );
        let config = adapter.apply_reasoning_config(&capability, ReasoningTier::Off).unwrap();
        assert_eq!(config["generationConfig"]["thinkingConfig"]["thinkingBudget"], 0);
    }

    #[test]
    fn extracts_budget_range_from_gemini_error() {
        assert_eq!(extract_thinking_budget_range("thinkingBudget must be between 0 and 8192"), Some((0, 8192)));
        assert_eq!(extract_thinking_budget_range("value must be between 1024 and 16384 tokens"), Some((1024, 16384)));
    }

    /// 模型名里带 thinking / 2.5-pro 也不能凭名字判定能力。
    #[test]
    fn model_name_alone_yields_no_hints() {
        let adapter = GeminiAdapter;
        let hints = adapter.metadata_hints(&json!({ "name": "models/gemini-2.5-pro-thinking-exp" }));
        assert!(hints.reasoning_fields.is_empty());
        assert!(hints.budget_range.is_none());
        assert!(!hints.cannot_disable);
    }

    #[test]
    fn reads_declared_budget_range_and_disable_flag() {
        let adapter = GeminiAdapter;
        let hints = adapter.metadata_hints(&json!({
            "name": "models/anything",
            "thinkingConfig": { "thinkingBudgetMin": 128, "thinkingBudgetMax": 24576, "canDisableThinking": false }
        }));
        assert_eq!(hints.budget_range, Some((128, 24576)));
        assert!(hints.cannot_disable);
        assert_eq!(hints.dynamic_sentinel, Some(-1));
    }

    #[test]
    fn probe_targets_generate_content_for_the_given_model() {
        let adapter = GeminiAdapter;
        let probe = adapter.validation_probe("models/some-model").expect("probe missing");
        assert_eq!(probe.endpoint, "/v1beta/models/some-model:generateContent");
        assert_eq!(probe.body["generationConfig"]["thinkingConfig"]["thinkingBudget"], 999_999);
        // 这条 probe 原本完全没有输出上限，是本次成本控制的重点修复对象。
        assert_eq!(
            probe.output_limits,
            vec![super::super::OutputLimitPatch::at("/generationConfig/maxOutputTokens")]
        );
    }

    #[test]
    fn extracts_thinking_tokens_from_usage_metadata() {
        let adapter = GeminiAdapter;
        let (input, output) = adapter.observe_usage(&json!({
            "usageMetadata": { "promptTokenCount": 10, "candidatesTokenCount": 20, "thinkingTokenCount": 64 }
        }));
        assert_eq!(input, None);
        assert_eq!(output, Some(64));
    }

    #[test]
    fn detects_thinking_tokens_in_response() {
        let adapter = GeminiAdapter;
        let response = json!({
            "candidates": [{"content": {"parts": [{"text": "OK"}]}}],
            "usageMetadata": {
                "promptTokenCount": 10,
                "candidatesTokenCount": 5,
                "thinkingTokenCount": 32
            }
        });
        assert!(adapter.has_reasoning_in_response(&response));
    }

    #[test]
    fn rejects_response_without_thinking_tokens() {
        let adapter = GeminiAdapter;
        let response = json!({
            "candidates": [{"content": {"parts": [{"text": "OK"}]}}],
            "usageMetadata": {
                "promptTokenCount": 10,
                "candidatesTokenCount": 5
            }
        });
        assert!(!adapter.has_reasoning_in_response(&response));
    }

    #[test]
    fn rejects_response_with_zero_thinking_tokens() {
        let adapter = GeminiAdapter;
        let response = json!({
            "candidates": [{"content": {"parts": [{"text": "OK"}]}}],
            "usageMetadata": {
                "promptTokenCount": 10,
                "candidatesTokenCount": 5,
                "thinkingTokenCount": 0
            }
        });
        assert!(!adapter.has_reasoning_in_response(&response));
    }

    #[test]
    fn rejects_response_without_usage_metadata() {
        let adapter = GeminiAdapter;
        let response = json!({
            "candidates": [{"content": {"parts": [{"text": "OK"}]}}]
        });
        assert!(!adapter.has_reasoning_in_response(&response));
    }
}
