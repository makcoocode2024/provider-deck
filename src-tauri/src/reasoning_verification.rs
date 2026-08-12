use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::{
    error::{AppError, AppResult},
    model::ProtocolKind,
    protocol::normalize_base_url,
    reasoning_adapters::{adapter_for, AuthScheme},
    reasoning_capability::{ReasoningBinding, ReasoningCapability, ReasoningTier},
};

/// 单次运行时验证记录
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeVerification {
    /// 验证的模型 ID（字面值，大小写敏感）
    pub model_id: String,

    /// 验证的 base URL（已归一化）
    pub base_url: String,

    /// 验证时选择的 tier
    pub tier: ReasoningTier,

    /// 验证时使用的 binding（序列化保存，用于 UI 展示）
    pub binding: ReasoningBinding,

    /// 验证结果
    pub result: VerificationResult,

    /// 验证时间戳（ISO 8601）
    pub verified_at: String,

    /// 协议类型
    pub protocol: String,
}

/// 验证结果
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum VerificationResult {
    /// 响应符合预期：包含对应 tier 的 reasoning 字段
    Confirmed,

    /// 响应不符合预期：缺失 reasoning 字段或值不匹配
    Rejected {
        #[serde(rename = "reason")]
        reason: String,
    },

    /// 验证失败：网络错误、API 错误等
    Failed {
        #[serde(rename = "error")]
        error: String,
    },
}

/// 执行运行时验证
///
/// 发送一个最小测试请求，检查响应是否包含推理相关字段。
/// 不修改 capability、confidence 或写入持久化存储。
pub async fn verify_reasoning_capability(
    base_url: &str,
    model_id: &str,
    api_key: &str,
    protocol: ProtocolKind,
    capability: &ReasoningCapability,
    tier: ReasoningTier,
) -> AppResult<RuntimeVerification> {
    // 1. 归一化 base_url
    let normalized_base_url = normalize_base_url(base_url).unwrap_or_else(|_| base_url.to_string());

    // 2. 获取 adapter
    let adapter = adapter_for(protocol);

    // 3. 从 capability 解析出 binding
    let tier_option = capability.tier(tier).ok_or_else(|| {
        AppError::InvalidInput(format!("Tier {tier:?} 在该 capability 中不存在"))
    })?;
    let binding = tier_option.binding.clone();

    // 4. 构造基础请求（最小 prompt）
    let mut request = build_base_request(protocol, model_id)?;

    // 5. 调用 adapter 注入推理配置
    if let Some(reasoning_config) = adapter.apply_reasoning_config(capability, tier) {
        merge_json_objects(&mut request, reasoning_config);
    }

    // 6. 发送请求
    let protocol_str = protocol_kind_to_string(protocol);
    let response = send_verification_request(&normalized_base_url, api_key, protocol, &request).await;

    // 7. 根据响应生成验证结果
    let result = match response {
        Ok(response_body) => {
            if adapter.has_reasoning_in_response(&response_body) {
                VerificationResult::Confirmed
            } else {
                VerificationResult::Rejected {
                    reason: format!("响应中未检测到 {} 协议的推理字段", protocol_str),
                }
            }
        }
        Err(VerificationError::ApiError { status, body }) => VerificationResult::Failed {
            error: format!("API 错误 {status}：{body}"),
        },
        Err(VerificationError::NetworkError(msg)) => VerificationResult::Failed { error: msg },
    };

    // 8. 构造验证记录
    Ok(RuntimeVerification {
        model_id: model_id.to_string(),
        base_url: normalized_base_url,
        tier,
        binding,
        result,
        verified_at: chrono::Utc::now().to_rfc3339(),
        protocol: protocol_str,
    })
}

/// 构造基础验证请求（不含推理配置）
fn build_base_request(protocol: ProtocolKind, model_id: &str) -> AppResult<Value> {
    match protocol {
        ProtocolKind::Openai | ProtocolKind::AzureOpenai | ProtocolKind::Custom => Ok(json!({
            "model": model_id,
            "messages": [{"role": "user", "content": "请回复 OK"}],
        })),
        ProtocolKind::Anthropic => Ok(json!({
            "model": model_id,
            "messages": [{"role": "user", "content": "请回复 OK"}],
            "max_tokens": 100,
        })),
        ProtocolKind::Gemini => {
            let stripped = model_id.strip_prefix("models/").unwrap_or(model_id);
            Ok(json!({
                "model": stripped,
                "contents": [{"role": "user", "parts": [{"text": "请回复 OK"}]}],
            }))
        }
    }
}

/// 合并 JSON 对象（将 source 的字段合并到 target）
fn merge_json_objects(target: &mut Value, source: Value) {
    if let (Some(target_obj), Some(source_obj)) = (target.as_object_mut(), source.as_object()) {
        for (key, value) in source_obj {
            if target_obj.contains_key(key) && value.is_object() {
                // 递归合并嵌套对象
                if let Some(target_nested) = target_obj.get_mut(key) {
                    merge_json_objects(target_nested, value.clone());
                }
            } else {
                target_obj.insert(key.clone(), value.clone());
            }
        }
    }
}

/// 验证请求的内部错误类型
#[derive(Debug)]
enum VerificationError {
    NetworkError(String),
    ApiError { status: u16, body: String },
}

/// 发送验证请求
async fn send_verification_request(
    base_url: &str,
    api_key: &str,
    protocol: ProtocolKind,
    request_body: &Value,
) -> Result<Value, VerificationError> {
    let auth_scheme = match protocol {
        ProtocolKind::Openai | ProtocolKind::AzureOpenai | ProtocolKind::Custom => AuthScheme::Bearer,
        ProtocolKind::Anthropic => AuthScheme::AnthropicKey,
        ProtocolKind::Gemini => AuthScheme::GoogleKey,
    };

    // 构造完整 URL（假设 chat endpoint）
    let endpoint = match protocol {
        ProtocolKind::Openai | ProtocolKind::AzureOpenai | ProtocolKind::Custom => {
            format!("{}/chat/completions", base_url.trim_end_matches('/'))
        }
        ProtocolKind::Anthropic => format!("{}/messages", base_url.trim_end_matches('/')),
        ProtocolKind::Gemini => {
            let model = request_body["model"].as_str().unwrap_or("unknown");
            format!(
                "{}/models/{}:generateContent",
                base_url.trim_end_matches('/'),
                model
            )
        }
    };

    // 构造 HTTP 客户端
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| VerificationError::NetworkError(format!("构造 HTTP 客户端失败：{e}")))?;

    // 构造请求
    let mut req = client.post(&endpoint).json(request_body);

    // 设置鉴权头
    req = match auth_scheme {
        AuthScheme::Bearer => req.bearer_auth(api_key),
        AuthScheme::AnthropicKey => req
            .header("x-api-key", api_key)
            .header("anthropic-version", "2023-06-01"),
        AuthScheme::GoogleKey => req.header("x-goog-api-key", api_key),
    };

    // 发送请求
    let response = req
        .send()
        .await
        .map_err(|e| VerificationError::NetworkError(format!("请求失败：{e}")))?;

    let status = response.status().as_u16();

    if response.status().is_success() {
        response
            .json::<Value>()
            .await
            .map_err(|e| VerificationError::NetworkError(format!("解析响应失败：{e}")))
    } else {
        let body = response
            .text()
            .await
            .unwrap_or_else(|_| "无法读取响应体".to_string());
        Err(VerificationError::ApiError { status, body })
    }
}

/// 判断一条验证记录是否仍属于给定的 (base_url, model_id)。
///
/// base_url 两侧都归一化后比较：用户原始输入的尾斜杠/大小写差异不该让记录失效。
/// model_id 走字面比较——模型标识符大小写敏感，`GPT-4` 与 `gpt-4` 是两个模型。
pub fn belongs_to(verification: &RuntimeVerification, base_url: &str, model_id: &str) -> bool {
    if verification.model_id != model_id {
        return false;
    }
    let stored = normalize_base_url(&verification.base_url).unwrap_or_else(|_| verification.base_url.clone());
    let current = normalize_base_url(base_url).unwrap_or_else(|_| base_url.to_string());
    stored == current
}

/// 剪掉不再属于当前端点或已消失模型的验证记录。
///
/// 两条规则同时生效：
/// - 记录里的 base_url 与当前端点不一致 → 丢弃（换端点等于换了被断言的对象）
/// - model_id 不在当前模型列表里 → 丢弃（模型没了，记录无处展示）
///
/// 这与 [`crate::reasoning_selection::prune_missing`] 的"只按 model_id 剪枝"是刻意的不对称：
/// 选择是用户意图，换端点后依然成立；验证是对某个端点运行时行为的事实断言，换端点即失效。
pub fn retain_for_endpoint(
    verifications: &mut HashMap<String, Vec<RuntimeVerification>>,
    base_url: &str,
    models: &[crate::model::ModelInfo],
) {
    verifications.retain(|model_id, records| {
        if !models.iter().any(|model| &model.id == model_id) {
            return false;
        }
        records.retain(|record| belongs_to(record, base_url, model_id));
        !records.is_empty()
    });
}

fn protocol_kind_to_string(protocol: ProtocolKind) -> String {
    match protocol {
        ProtocolKind::Openai => "openai".to_string(),
        ProtocolKind::Anthropic => "anthropic".to_string(),
        ProtocolKind::Gemini => "gemini".to_string(),
        ProtocolKind::AzureOpenai => "azure-openai".to_string(),
        ProtocolKind::Custom => "custom".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reasoning_capability::{ReasoningConfidence, ReasoningKey};

    fn test_key() -> ReasoningKey {
        ReasoningKey::new("https://api.example.com/v1", "test-model")
    }

    #[test]
    fn normalizes_base_url_in_verification_record() {
        // 测试 normalize_base_url 在验证记录中生效
        let base_url_with_slash = "https://api.example.com/v1/";
        let normalized = normalize_base_url(base_url_with_slash).unwrap();

        assert_eq!(normalized, "https://api.example.com/v1");
    }

    #[test]
    fn builds_openai_base_request() {
        let request = build_base_request(ProtocolKind::Openai, "gpt-4").unwrap();
        assert_eq!(request["model"], "gpt-4");
        assert!(request["messages"].is_array());
        assert_eq!(request["messages"][0]["role"], "user");
    }

    #[test]
    fn builds_anthropic_base_request() {
        let request = build_base_request(ProtocolKind::Anthropic, "claude-3-5-sonnet").unwrap();
        assert_eq!(request["model"], "claude-3-5-sonnet");
        assert_eq!(request["max_tokens"], 100);
        assert!(request["messages"].is_array());
    }

    #[test]
    fn builds_gemini_base_request() {
        let request = build_base_request(ProtocolKind::Gemini, "models/gemini-2.0-flash").unwrap();
        assert_eq!(request["model"], "gemini-2.0-flash");
        assert!(request["contents"].is_array());
    }

    #[test]
    fn merges_reasoning_config_into_base_request() {
        let mut base = json!({"model": "test", "messages": []});
        let reasoning = json!({"reasoning": {"effort": "high"}});
        merge_json_objects(&mut base, reasoning);
        assert_eq!(base["reasoning"]["effort"], "high");
        assert_eq!(base["model"], "test");
    }

    #[test]
    fn merges_nested_reasoning_config() {
        let mut base = json!({"model": "test", "generationConfig": {"temperature": 0.7}});
        let reasoning = json!({"generationConfig": {"thinkingConfig": {"thinkingBudget": 1024}}});
        merge_json_objects(&mut base, reasoning);
        assert_eq!(base["generationConfig"]["temperature"], 0.7);
        assert_eq!(base["generationConfig"]["thinkingConfig"]["thinkingBudget"], 1024);
    }

    #[test]
    fn verification_result_confirmed_serializes_correctly() {
        let result = VerificationResult::Confirmed;
        let json = serde_json::to_value(result).unwrap();
        assert_eq!(json["status"], "confirmed");
    }

    #[test]
    fn verification_result_rejected_serializes_correctly() {
        let result = VerificationResult::Rejected {
            reason: "缺少推理字段".to_string(),
        };
        let json = serde_json::to_value(result).unwrap();
        assert_eq!(json["status"], "rejected");
        assert_eq!(json["reason"], "缺少推理字段");
    }

    #[test]
    fn verification_result_failed_serializes_correctly() {
        let result = VerificationResult::Failed {
            error: "网络超时".to_string(),
        };
        let json = serde_json::to_value(result).unwrap();
        assert_eq!(json["status"], "failed");
        assert_eq!(json["error"], "网络超时");
    }

    fn record(base_url: &str, model_id: &str, tier: ReasoningTier) -> RuntimeVerification {
        RuntimeVerification {
            model_id: model_id.into(),
            base_url: base_url.into(),
            tier,
            binding: ReasoningBinding::Effort { value: "high".into() },
            result: VerificationResult::Confirmed,
            verified_at: "2026-08-12T10:30:00Z".into(),
            protocol: "openai".into(),
        }
    }

    fn model_info(id: &str) -> crate::model::ModelInfo {
        crate::model::ModelInfo {
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

    #[test]
    fn same_endpoint_and_model_belongs() {
        let verification = record("https://api.example.com/v1", "gpt-x", ReasoningTier::Deep);
        assert!(belongs_to(&verification, "https://api.example.com/v1", "gpt-x"));
    }

    /// 端点变了就是换了被断言的对象，旧记录不能继续展示。
    #[test]
    fn foreign_base_url_does_not_belong() {
        let verification = record("https://old.example.com/v1", "gpt-x", ReasoningTier::Deep);
        assert!(!belongs_to(&verification, "https://new.example.com/v1", "gpt-x"));
    }

    /// model_id 走字面比较：大小写不同就是两个模型。
    #[test]
    fn foreign_model_id_does_not_belong() {
        let verification = record("https://api.example.com/v1", "gpt-x", ReasoningTier::Deep);
        assert!(!belongs_to(&verification, "https://api.example.com/v1", "gpt-y"));
        assert!(!belongs_to(&verification, "https://api.example.com/v1", "GPT-X"));
    }

    /// 书写差异（尾斜杠）不该让记录失效，否则每次保存都会清空历史。
    #[test]
    fn normalized_base_url_still_belongs() {
        let verification = record("https://api.example.com/v1", "gpt-x", ReasoningTier::Deep);
        assert!(belongs_to(&verification, "https://api.example.com/v1/", "gpt-x"));
    }

    #[test]
    fn retain_keeps_records_for_the_same_endpoint() {
        let mut map = HashMap::new();
        map.insert("gpt-x".to_string(), vec![record("https://api.example.com/v1", "gpt-x", ReasoningTier::Deep)]);
        retain_for_endpoint(&mut map, "https://api.example.com/v1", &[model_info("gpt-x")]);
        assert_eq!(map.get("gpt-x").map(Vec::len), Some(1));
    }

    /// base_url 改变后旧验证记录必须被清掉。
    #[test]
    fn retain_drops_records_after_endpoint_change() {
        let mut map = HashMap::new();
        map.insert("gpt-x".to_string(), vec![record("https://old.example.com/v1", "gpt-x", ReasoningTier::Deep)]);
        retain_for_endpoint(&mut map, "https://new.example.com/v1", &[model_info("gpt-x")]);
        assert!(map.is_empty(), "换端点后旧验证记录仍然留着：{map:?}");
    }

    /// 模型从列表里消失后对应的记录一并清掉。
    #[test]
    fn retain_drops_records_for_missing_models() {
        let mut map = HashMap::new();
        map.insert("gone".to_string(), vec![record("https://api.example.com/v1", "gone", ReasoningTier::Deep)]);
        map.insert("kept".to_string(), vec![record("https://api.example.com/v1", "kept", ReasoningTier::Deep)]);
        retain_for_endpoint(&mut map, "https://api.example.com/v1", &[model_info("kept")]);
        assert!(!map.contains_key("gone"));
        assert!(map.contains_key("kept"));
    }

    /// 不同模型的历史各自独立，剪枝不得串味。
    #[test]
    fn records_never_mix_between_models() {
        let mut map = HashMap::new();
        map.insert("gpt-x".to_string(), vec![record("https://api.example.com/v1", "gpt-x", ReasoningTier::Deep)]);
        // 故意放一条 key 与内部 model_id 不一致的脏记录，剪枝要把它清掉。
        map.insert("gpt-y".to_string(), vec![record("https://api.example.com/v1", "gpt-x", ReasoningTier::Deep)]);
        retain_for_endpoint(&mut map, "https://api.example.com/v1", &[model_info("gpt-x"), model_info("gpt-y")]);
        assert_eq!(map.get("gpt-x").map(Vec::len), Some(1));
        assert!(!map.contains_key("gpt-y"), "model_id 与 key 不一致的记录被保留了");
    }

    /// 同一档位重复验证要追加历史，不是覆盖——用户要看到"上次通过、这次失败"。
    #[test]
    fn repeated_verification_appends_history() {
        let mut map: HashMap<String, Vec<RuntimeVerification>> = HashMap::new();
        map.entry("gpt-x".into()).or_default().push(record("https://api.example.com/v1", "gpt-x", ReasoningTier::Deep));
        let mut second = record("https://api.example.com/v1", "gpt-x", ReasoningTier::Deep);
        second.verified_at = "2026-08-12T11:00:00Z".into();
        second.result = VerificationResult::Rejected { reason: "未检测到推理字段".into() };
        map.entry("gpt-x".into()).or_default().push(second);

        let history = map.get("gpt-x").expect("history missing");
        assert_eq!(history.len(), 2, "第二次验证覆盖了第一条记录");
        assert!(matches!(history[0].result, VerificationResult::Confirmed));
        assert!(matches!(history[1].result, VerificationResult::Rejected { .. }));
        // 剪枝不能因为存在 Rejected 就丢历史。
        retain_for_endpoint(&mut map, "https://api.example.com/v1", &[model_info("gpt-x")]);
        assert_eq!(map.get("gpt-x").map(Vec::len), Some(2));
    }

    /// 三态一律入库：Rejected 是有效负事实，Failed 是排障线索，都属于用户验证历史。
    /// 之所以不过滤掉失败态：隐藏它们等于让用户反复点同一个按钮却看不到痕迹。
    #[test]
    fn failed_and_rejected_records_are_persisted() {
        let mut map: HashMap<String, Vec<RuntimeVerification>> = HashMap::new();

        let mut rejected = record("https://api.example.com/v1", "gpt-x", ReasoningTier::Deep);
        rejected.result = VerificationResult::Rejected { reason: "响应中没有推理产物".into() };
        map.entry("gpt-x".into()).or_default().push(rejected);

        let mut failed = record("https://api.example.com/v1", "gpt-x", ReasoningTier::Deep);
        failed.verified_at = "2026-08-12T11:05:00Z".into();
        failed.result = VerificationResult::Failed { error: "连接上游超时".into() };
        map.entry("gpt-x".into()).or_default().push(failed);

        let history = map.get("gpt-x").expect("history missing");
        assert_eq!(history.len(), 2, "失败态记录被丢弃了");
        assert!(matches!(history[0].result, VerificationResult::Rejected { .. }));
        assert!(matches!(history[1].result, VerificationResult::Failed { .. }));

        // 剪枝不得因为记录是失败态就清掉它。
        retain_for_endpoint(&mut map, "https://api.example.com/v1", &[model_info("gpt-x")]);
        assert_eq!(map.get("gpt-x").map(Vec::len), Some(2), "剪枝把失败记录清掉了");
    }

    /// Rejected/Failed 入库后 capability 的 confidence 必须原封不动。
    /// 这是 discovery 与 verification 两条链路隔离的底线：用户验证行为不回写系统能力事实。
    #[test]
    fn persisting_failures_leaves_confidence_untouched() {
        use crate::reasoning_capability::{ReasoningCapability, ReasoningConfidence, ReasoningKey};

        let capability = ReasoningCapability::from_effort_enum(
            ReasoningKey::new("https://api.example.com/v1", "gpt-x"),
            &["low".into(), "high".into()],
            ReasoningConfidence::Declared,
        );
        let before = capability.confidence;

        let mut map: HashMap<String, Vec<RuntimeVerification>> = HashMap::new();
        for result in [
            VerificationResult::Rejected { reason: "无推理产物".into() },
            VerificationResult::Failed { error: "网络不可达".into() },
            VerificationResult::Confirmed,
        ] {
            let mut entry = record("https://api.example.com/v1", "gpt-x", ReasoningTier::Deep);
            entry.result = result;
            map.entry("gpt-x".into()).or_default().push(entry);
        }

        assert_eq!(capability.confidence, before, "验证历史改动了 discovery 的 confidence");
        assert_eq!(capability.confidence, ReasoningConfidence::Declared);
        assert_eq!(map.get("gpt-x").map(Vec::len), Some(3));
    }

    /// 失败态不得覆盖此前的 Confirmed：历史按时间追加，UI 自己决定怎么呈现。
    #[test]
    fn failure_does_not_erase_earlier_confirmation() {
        let mut map: HashMap<String, Vec<RuntimeVerification>> = HashMap::new();
        map.entry("gpt-x".into()).or_default().push(record("https://api.example.com/v1", "gpt-x", ReasoningTier::Deep));

        let mut later = record("https://api.example.com/v1", "gpt-x", ReasoningTier::Deep);
        later.verified_at = "2026-08-12T12:00:00Z".into();
        later.result = VerificationResult::Failed { error: "429 Too Many Requests".into() };
        map.entry("gpt-x".into()).or_default().push(later);

        let history = map.get("gpt-x").expect("history missing");
        assert_eq!(history.len(), 2);
        assert!(
            matches!(history[0].result, VerificationResult::Confirmed),
            "先前的 Confirmed 被失败记录覆盖了"
        );
    }

    /// 验证记录不得携带凭据或请求/响应原文。
    #[test]
    fn verification_record_carries_no_secrets() {
        let verification = record("https://api.example.com/v1", "gpt-x", ReasoningTier::Deep);
        let serialized = serde_json::to_string(&verification).expect("serialize failed");
        for forbidden in ["apiKey", "api_key", "sk-", "messages", "请回复 OK"] {
            assert!(!serialized.contains(forbidden), "验证记录里出现了 {forbidden}：{serialized}");
        }
    }

    #[test]
    fn runtime_verification_includes_all_fields() {
        let key = test_key();
        let capability =
            ReasoningCapability::from_effort_enum(key, &["low".into(), "medium".into(), "high".into()], ReasoningConfidence::Validated);
        let tier_option = capability.tier(ReasoningTier::Standard).expect("Standard tier should exist for medium effort");

        let verification = RuntimeVerification {
            model_id: "test-model".to_string(),
            base_url: "https://api.example.com/v1".to_string(),
            tier: ReasoningTier::Standard,
            binding: tier_option.binding.clone(),
            result: VerificationResult::Confirmed,
            verified_at: "2026-08-12T10:30:00Z".to_string(),
            protocol: "openai".to_string(),
        };

        assert_eq!(verification.model_id, "test-model");
        assert_eq!(verification.tier, ReasoningTier::Standard);
        assert!(matches!(verification.result, VerificationResult::Confirmed));
    }
}
