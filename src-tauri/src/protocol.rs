use std::time::{Duration, Instant};
use reqwest::{header::{AUTHORIZATION, CONTENT_TYPE}, Client, StatusCode};
use serde_json::{json, Value};
use url::Url;
use crate::{error::{AppError, AppResult}, model::{AppSettings, CodexCompatibility, ModelInfo, ProbeResult, ProtocolKind, ProviderDraft, ProviderTestCheck, ProviderTestReport}, redaction::redact};

pub fn normalize_base_url(input: &str) -> AppResult<String> {
    let trimmed = input.trim();
    if trimmed.is_empty() { return Err(AppError::InvalidInput("请输入 Base URL".into())); }
    let candidate = if trimmed.contains("://") { trimmed.to_owned() } else { format!("https://{trimmed}") };
    let mut url = Url::parse(&candidate).map_err(|_| AppError::InvalidInput("Base URL 格式无效".into()))?;
    if !matches!(url.scheme(), "http" | "https") { return Err(AppError::InvalidInput("仅支持 HTTP 或 HTTPS".into())); }
    if !url.username().is_empty() || url.password().is_some() { return Err(AppError::InvalidInput("Base URL 不能包含凭据".into())); }
    if url.query().is_some() || url.fragment().is_some() { return Err(AppError::InvalidInput("Base URL 不能包含查询参数或片段".into())); }
    let normalized_path = url.path().trim_end_matches('/').to_owned();
    url.set_path(if normalized_path.is_empty() { "/" } else { &normalized_path });
    Ok(url.as_str().trim_end_matches('/').to_owned())
}

fn endpoint(base: &str, suffix: &str, known_prefix: &str) -> AppResult<String> {
    let mut url = Url::parse(base).map_err(|_| AppError::InvalidInput("Base URL 格式无效".into()))?;
    let path = url.path().trim_end_matches('/');
    let next = if path.ends_with(known_prefix) { format!("{path}/{suffix}") } else { format!("{path}{known_prefix}/{suffix}") };
    url.set_path(&next.replace("//", "/"));
    Ok(url.to_string())
}

fn build_client(settings: &AppSettings, timeout: u64) -> AppResult<Client> {
    let mut builder = Client::builder()
        .timeout(Duration::from_secs(timeout.clamp(3, 120)))
        .danger_accept_invalid_certs(settings.allow_self_signed_certificates);
    if !settings.proxy_url.trim().is_empty() {
        builder = builder.proxy(reqwest::Proxy::all(settings.proxy_url.trim()).map_err(|e| AppError::InvalidInput(format!("代理地址无效：{e}")))?);
    }
    builder.build().map_err(|e| AppError::Network(e.to_string()))
}

pub async fn fetch_models(draft: &ProviderDraft, settings: &AppSettings) -> AppResult<(String, Vec<ModelInfo>, f64)> {
    let base = normalize_base_url(&draft.base_url)?;
    let client = build_client(settings, draft.timeout_seconds)?;
    let result = match draft.protocol_hint.as_ref().unwrap_or(&ProtocolKind::Openai) {
        ProtocolKind::Openai | ProtocolKind::Custom => probe_openai(&client, &base, &draft.api_key).await,
        ProtocolKind::Anthropic => probe_anthropic(&client, &base, &draft.api_key).await,
        ProtocolKind::Gemini => probe_gemini(&client, &base, &draft.api_key).await,
        ProtocolKind::AzureOpenai => return Err(AppError::InvalidInput("Azure OpenAI 无法通过通用接口枚举部署，请在编辑服务时手动填写部署模型。".into())),
    };
    result.map_err(|(_, detail)| AppError::Network(redact(&detail, &[&draft.api_key])))
}

pub async fn test_conversation(provider_id: String, draft: &ProviderDraft, selected_model: Option<String>, settings: &AppSettings) -> AppResult<ProviderTestReport> {
    let started = Instant::now();
    let model = selected_model.filter(|value| !value.trim().is_empty()).or_else(|| draft.default_model.clone());
    let mut report = ProviderTestReport {
        provider_id,
        model: model.clone(),
        total_latency_ms: 0,
        checks: Vec::new(),
        reply_preview: None,
    };
    let models_started = Instant::now();
    match fetch_models(draft, settings).await {
        Ok((target, models, _)) => report.checks.push(ProviderTestCheck {
            id: "connectivity".into(),
            label: "连通性与身份验证".into(),
            status: "passed".into(),
            detail: format!("模型接口可访问，共返回 {} 个模型：{}", models.len(), target),
            latency_ms: Some(models_started.elapsed().as_millis() as u64),
        }),
        Err(error) => {
            report.checks.push(ProviderTestCheck {
                id: "connectivity".into(),
                label: "连通性与身份验证".into(),
                status: "failed".into(),
                detail: error.to_string(),
                latency_ms: Some(models_started.elapsed().as_millis() as u64),
            });
            report.total_latency_ms = started.elapsed().as_millis() as u64;
            return Ok(report);
        }
    }
    let Some(model) = model else {
        report.checks.push(ProviderTestCheck {
            id: "conversation".into(), label: "最小真实对话".into(), status: "failed".into(),
            detail: "没有可用于测试的模型，请先获取模型或在编辑服务时选择默认模型。".into(), latency_ms: None,
        });
        report.total_latency_ms = started.elapsed().as_millis() as u64;
        return Ok(report);
    };
    let client = build_client(settings, draft.timeout_seconds)?;
    let base = normalize_base_url(&draft.base_url)?;
    let conversation_started = Instant::now();
    let result = match draft.protocol_hint.as_ref().unwrap_or(&ProtocolKind::Openai) {
        ProtocolKind::Openai | ProtocolKind::Custom => test_openai_conversation(&client, &base, &draft.api_key, &model).await,
        ProtocolKind::Anthropic => test_anthropic_conversation(&client, &base, &draft.api_key, &model).await,
        ProtocolKind::Gemini => test_gemini_conversation(&client, &base, &draft.api_key, &model).await,
        ProtocolKind::AzureOpenai => Err("Azure OpenAI 需要明确的部署路径，当前无法执行通用真实对话测试。".into()),
    };
    let latency = conversation_started.elapsed().as_millis() as u64;
    match result {
        Ok((target, reply)) => {
            let preview = reply.trim().chars().take(160).collect::<String>();
            report.reply_preview = (!preview.is_empty()).then_some(preview);
            report.checks.push(ProviderTestCheck {
                id: "conversation".into(), label: "最小真实对话".into(), status: "passed".into(),
                detail: format!("模型已成功生成回复：{target}"), latency_ms: Some(latency),
            });
        }
        Err(detail) => report.checks.push(ProviderTestCheck {
            id: "conversation".into(), label: "最小真实对话".into(), status: "failed".into(),
            detail: redact(&detail, &[&draft.api_key]), latency_ms: Some(latency),
        }),
    }
    report.total_latency_ms = started.elapsed().as_millis() as u64;
    Ok(report)
}

async fn checked_json(response: reqwest::Response) -> Result<Value, String> {
    let status = response.status();
    let body = response.text().await.map_err(|error| format!("读取响应失败：{error}"))?;
    let value = serde_json::from_str::<Value>(&body).map_err(|error| format!("响应不是有效 JSON：{error}"))?;
    if status.is_success() { Ok(value) } else { Err(classify_status(status, &value)) }
}

fn openai_reply(body: &Value) -> Option<String> {
    let content = body.pointer("/choices/0/message/content")?;
    if let Some(text) = content.as_str() { return Some(text.to_owned()); }
    content.as_array().map(|parts| parts.iter().filter_map(|part| part.get("text").and_then(Value::as_str)).collect::<Vec<_>>().join(""))
}

async fn test_openai_conversation(client: &Client, base: &str, key: &str, model: &str) -> Result<(String, String), String> {
    let target = endpoint(base, "chat/completions", "/v1").map_err(|error| error.to_string())?;
    let response = client.post(&target).header(AUTHORIZATION, format!("Bearer {key}")).header(CONTENT_TYPE, "application/json")
        .header("x-provider-deck-probe", "minimal-conversation")
        .json(&json!({"model": model, "messages": [{"role": "user", "content": "Reply with OK only."}], "max_tokens": 8, "stream": false}))
        .send().await.map_err(|error| classify_reqwest(&error))?;
    let body = checked_json(response).await?;
    let reply = openai_reply(&body).ok_or_else(|| "响应缺少 choices[0].message.content".to_string())?;
    Ok((target, reply))
}

async fn test_anthropic_conversation(client: &Client, base: &str, key: &str, model: &str) -> Result<(String, String), String> {
    let target = endpoint(base, "messages", "/v1").map_err(|error| error.to_string())?;
    let response = client.post(&target).header("x-api-key", key).header("anthropic-version", "2023-06-01").header(CONTENT_TYPE, "application/json")
        .header("x-provider-deck-probe", "minimal-conversation")
        .json(&json!({"model": model, "messages": [{"role": "user", "content": "Reply with OK only."}], "max_tokens": 8, "stream": false}))
        .send().await.map_err(|error| classify_reqwest(&error))?;
    let body = checked_json(response).await?;
    let reply = body.get("content").and_then(Value::as_array).map(|parts| parts.iter().filter_map(|part| part.get("text").and_then(Value::as_str)).collect::<Vec<_>>().join(""))
        .filter(|value| !value.is_empty()).ok_or_else(|| "响应缺少 content 文本".to_string())?;
    Ok((target, reply))
}

async fn test_gemini_conversation(client: &Client, base: &str, key: &str, model: &str) -> Result<(String, String), String> {
    let models_target = endpoint(base, "models", "/v1beta").map_err(|error| error.to_string())?;
    let mut url = Url::parse(&models_target).map_err(|error| error.to_string())?;
    url.path_segments_mut().map_err(|_| "Base URL 无法追加模型路径".to_string())?.pop_if_empty().push(&format!("{model}:generateContent"));
    let target = url.to_string();
    let response = client.post(&target).header("x-goog-api-key", key).header(CONTENT_TYPE, "application/json")
        .header("x-provider-deck-probe", "minimal-conversation")
        .json(&json!({"contents": [{"role": "user", "parts": [{"text": "Reply with OK only."}]}], "generationConfig": {"maxOutputTokens": 8}}))
        .send().await.map_err(|error| classify_reqwest(&error))?;
    let body = checked_json(response).await?;
    let reply = body.pointer("/candidates/0/content/parts").and_then(Value::as_array).map(|parts| parts.iter().filter_map(|part| part.get("text").and_then(Value::as_str)).collect::<Vec<_>>().join(""))
        .filter(|value| !value.is_empty()).ok_or_else(|| "响应缺少 candidates[0].content.parts 文本".to_string())?;
    Ok((target, reply))
}

fn context_window(item: &Value) -> Option<u64> {
    [
        "/context_window",
        "/contextWindow",
        "/context_length",
        "/contextLength",
        "/max_context_length",
        "/maxContextLength",
        "/max_input_tokens",
        "/inputTokenLimit",
        "/limits/context_window",
        "/limits/context_length",
        "/metadata/context_window",
        "/capabilities/context_window",
    ]
    .iter()
    .find_map(|pointer| item.pointer(pointer).and_then(|value| value.as_u64().or_else(|| value.as_str()?.parse().ok())))
}

fn parameter_count_billions(item: &Value) -> Option<f64> {
    [
        "/parameter_count_billions",
        "/parameterCountBillions",
        "/parameters_billions",
        "/metadata/parameter_count_billions",
    ]
    .iter()
    .find_map(|pointer| {
        let value = item.pointer(pointer)?;
        value.as_f64().or_else(|| {
            value.as_str()?.trim().trim_end_matches(['B', 'b']).parse::<f64>().ok()
        })
    })
}

pub async fn probe(draft: &ProviderDraft, settings: &AppSettings) -> AppResult<ProbeResult> {
    let base = normalize_base_url(&draft.base_url)?;
    let url = Url::parse(&base).map_err(|_| AppError::InvalidInput("Base URL 格式无效".into()))?;
    if matches!(draft.protocol_hint, Some(ProtocolKind::AzureOpenai)) || url.host_str().is_some_and(|host| host.ends_with(".openai.azure.com")) {
        return Ok(ProbeResult {
            normalized_base_url: base,
            protocol: ProtocolKind::AzureOpenai,
            confidence: 0.82,
            models: Vec::new(),
            codex_compatibility: CodexCompatibility::Unknown,
            codex_probe_model: None,
            codex_probe_detail: Some("Azure OpenAI 需要部署名，未执行通用 Responses 工具探测。".into()),
            checked_endpoints: Vec::new(),
            user_message: "已识别 Azure OpenAI。部署名称无法从低权限公共接口可靠枚举，请手动填写模型部署名。".into(),
            technical_detail: None,
        });
    }
    let client = build_client(settings, draft.timeout_seconds)?;
    let order = match draft.protocol_hint {
        Some(ProtocolKind::Openai) => vec![ProtocolKind::Openai],
        Some(ProtocolKind::Anthropic) => vec![ProtocolKind::Anthropic],
        Some(ProtocolKind::Gemini) => vec![ProtocolKind::Gemini],
        _ => vec![ProtocolKind::Openai, ProtocolKind::Anthropic, ProtocolKind::Gemini],
    };
    let mut checked = Vec::new();
    let mut failures = Vec::new();
    for kind in order {
        let result = match kind {
            ProtocolKind::Openai => probe_openai(&client, &base, &draft.api_key).await,
            ProtocolKind::Anthropic => probe_anthropic(&client, &base, &draft.api_key).await,
            ProtocolKind::Gemini => probe_gemini(&client, &base, &draft.api_key).await,
            _ => continue,
        };
        match result {
            Ok((target, models, confidence)) => {
                checked.push(target);
                let is_openai = matches!(kind, ProtocolKind::Openai);
                let mut probe = ProbeResult {
                    normalized_base_url: base,
                    protocol: kind,
                    confidence,
                    models,
                    codex_compatibility: if is_openai { CodexCompatibility::Unknown } else { CodexCompatibility::NotApplicable },
                    codex_probe_model: None,
                    codex_probe_detail: None,
                    checked_endpoints: checked,
                    user_message: "模型列表读取成功。".into(),
                    technical_detail: None,
                };
                if is_openai {
                    refresh_selected_capabilities(draft, settings, &mut probe).await;
                    probe.user_message = codex_probe_message(&probe.codex_compatibility);
                } else {
                    if matches!(probe.protocol, ProtocolKind::Anthropic) {
                        refresh_selected_capabilities(draft, settings, &mut probe).await;
                    }
                    probe.user_message = "模型列表读取成功，未发起模型生成请求。".into();
                }
                return Ok(probe);
            }
            Err((target, message)) => { checked.push(target); failures.push(message); }
        }
    }
    let detail = redact(&failures.join("；"), &[&draft.api_key]);
    Err(AppError::Network(user_facing_failure(&detail)))
}

fn model_detail_endpoint(base: &str, model: &str) -> AppResult<String> {
    let models_url = endpoint(base, "models", "/v1")?;
    let mut url = Url::parse(&models_url).map_err(|_| AppError::InvalidInput("Base URL 格式无效".into()))?;
    url.path_segments_mut().map_err(|_| AppError::InvalidInput("Base URL 无法追加模型路径".into()))?.push(model);
    Ok(url.to_string())
}

fn push_checked(probe: &mut ProbeResult, target: String) {
    if !probe.checked_endpoints.contains(&target) { probe.checked_endpoints.push(target); }
}

pub async fn refresh_selected_capabilities(draft: &ProviderDraft, settings: &AppSettings, probe: &mut ProbeResult) {
    if !matches!(probe.protocol, ProtocolKind::Openai | ProtocolKind::Anthropic) { return; }
    let selected = draft.default_model.as_ref()
        .filter(|model| probe.models.iter().any(|item| &item.id == *model))
        .cloned()
        .or_else(|| probe.models.first().map(|model| model.id.clone()));
    let Some(model) = selected else {
        probe.codex_compatibility = CodexCompatibility::Unknown;
        probe.codex_probe_detail = Some("模型列表为空，无法执行 Codex 兼容性探测。".into());
        return;
    };
    let Ok(client) = build_client(settings, draft.timeout_seconds) else {
        probe.codex_compatibility = CodexCompatibility::Unknown;
        probe.codex_probe_detail = Some("无法创建安全网络客户端，已采用函数工具保守模式。".into());
        return;
    };

    if probe.models.iter().find(|item| item.id == model).and_then(|item| item.context_window).is_none() {
        if let Ok(target) = model_detail_endpoint(&probe.normalized_base_url, &model) {
            push_checked(probe, target.clone());
            let request = client.get(&target);
            let request = if matches!(probe.protocol, ProtocolKind::Anthropic) {
                request.header("x-api-key", &draft.api_key).header("anthropic-version", "2023-06-01")
            } else {
                request.header(AUTHORIZATION, format!("Bearer {}", draft.api_key))
            };
            if let Ok(response) = request.send().await {
                if response.status().is_success() {
                    if let Ok(body) = response.json::<Value>().await {
                        let detected = context_window(&body).or_else(|| body.get("data").and_then(context_window));
                        if let Some(model_info) = probe.models.iter_mut().find(|item| item.id == model) {
                            if detected.is_some() { model_info.context_window = detected; }
                        }
                    }
                }
            }
        }
    }

    if !matches!(probe.protocol, ProtocolKind::Openai) { return; }
    if probe.codex_probe_model.as_deref() == Some(model.as_str()) { return; }
    let target = match endpoint(&probe.normalized_base_url, "responses", "/v1") {
        Ok(target) => target,
        Err(error) => {
            probe.codex_compatibility = CodexCompatibility::Unknown;
            probe.codex_probe_detail = Some(error.to_string());
            return;
        }
    };
    push_checked(probe, target.clone());
    let (mut compatibility, mut detail) = probe_responses_tools(&client, &target, &draft.api_key, &model).await;
    if matches!(compatibility, CodexCompatibility::ResponsesUnsupported) {
        if let Ok(chat_target) = endpoint(&probe.normalized_base_url, "chat/completions", "/v1") {
            push_checked(probe, chat_target.clone());
            let (chat_supported, chat_detail) = probe_chat_tools(&client, &chat_target, &draft.api_key, &model).await;
            detail = format!("{detail} {chat_detail}");
            if chat_supported { compatibility = CodexCompatibility::ChatProxy; }
        }
    }
    probe.codex_compatibility = compatibility;
    probe.codex_probe_model = Some(model);
    probe.codex_probe_detail = Some(redact(&detail, &[&draft.api_key]));
}

async fn send_tool_probe(client: &Client, target: &str, key: &str, model: &str, tool: Value) -> Result<(StatusCode, String), String> {
    let payload = json!({
        "model": model,
        "input": "Provider Deck compatibility probe. Reply with OK.",
        "max_output_tokens": 1,
        "tool_choice": "none",
        "tools": [tool]
    });
    let response = client.post(target)
        .header(AUTHORIZATION, format!("Bearer {key}"))
        .header(CONTENT_TYPE, "application/json")
        .header("x-provider-deck-probe", "codex-tools")
        .json(&payload)
        .send().await.map_err(|error| classify_reqwest(&error))?;
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    Ok((status, body))
}

async fn probe_responses_tools(client: &Client, target: &str, key: &str, model: &str) -> (CodexCompatibility, String) {
    let custom_tool = json!({
        "type": "custom",
        "name": "provider_deck_probe",
        "description": "Compatibility probe; never call this tool.",
        "format": { "type": "text" }
    });
    match send_tool_probe(client, target, key, model, custom_tool).await {
        Ok((status, _)) if status.is_success() => return (CodexCompatibility::Full, "Responses API 接受 type=custom 工具。".into()),
        Ok((status, _body)) if matches!(status.as_u16(), 404 | 405 | 501) => {
            return (CodexCompatibility::ResponsesUnsupported, format!("Responses API 不可用（HTTP {}）。", status.as_u16()));
        }
        Ok((status, body)) if matches!(status.as_u16(), 401 | 403 | 429) || status.is_server_error() => {
            return (CodexCompatibility::Unknown, format!("Responses custom 探测未完成（HTTP {}）：{}", status.as_u16(), compact_error(&body)));
        }
        Err(error) => return (CodexCompatibility::Unknown, format!("Responses custom 探测失败：{error}")),
        Ok(_) => {}
    }

    let function_tool = json!({
        "type": "function",
        "name": "provider_deck_probe",
        "description": "Compatibility probe; never call this function.",
        "parameters": { "type": "object", "properties": {}, "additionalProperties": false }
    });
    match send_tool_probe(client, target, key, model, function_tool).await {
        Ok((status, _)) if status.is_success() => (CodexCompatibility::FunctionToolsOnly, "Responses API 可用，但 custom 工具被拒绝；将关闭自由格式补丁声明。".into()),
        Ok((status, _)) if matches!(status.as_u16(), 404 | 405 | 501) => (CodexCompatibility::ResponsesUnsupported, format!("Responses API 不可用（HTTP {}）。", status.as_u16())),
        Ok((status, body)) if status.is_client_error() => (CodexCompatibility::ResponsesUnsupported, format!("Responses API 无法处理 Codex 所需的 function 工具（HTTP {}）：{}", status.as_u16(), compact_error(&body))),
        Ok((status, body)) => (CodexCompatibility::Unknown, format!("Responses function 探测未完成（HTTP {}）：{}", status.as_u16(), compact_error(&body))),
        Err(error) => (CodexCompatibility::Unknown, format!("Responses function 探测失败：{error}")),
    }
}

async fn probe_chat_tools(client: &Client, target: &str, key: &str, model: &str) -> (bool, String) {
    let payload = json!({
        "model": model,
        "messages": [{ "role": "user", "content": "Provider Deck compatibility probe. Reply with OK." }],
        "max_tokens": 1,
        "tool_choice": "none",
        "tools": [{
            "type": "function",
            "function": {
                "name": "provider_deck_probe",
                "description": "Compatibility probe; never call this function.",
                "parameters": { "type": "object", "properties": {}, "additionalProperties": false }
            }
        }]
    });
    let response = client.post(target)
        .header(AUTHORIZATION, format!("Bearer {key}"))
        .header(CONTENT_TYPE, "application/json")
        .header("x-provider-deck-probe", "chat-function-tools")
        .json(&payload)
        .send().await;
    match response {
        Ok(response) => {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            if status.is_success() {
                (true, "Chat Completions 与标准 function 工具探测通过，将使用本机 Responses 兼容桥。".into())
            } else {
                (false, format!("Chat Completions function 探测失败（HTTP {}）：{}", status.as_u16(), compact_error(&body)))
            }
        }
        Err(error) => (false, format!("Chat Completions function 探测失败：{}", classify_reqwest(&error))),
    }
}

fn compact_error(body: &str) -> String {
    let parsed = serde_json::from_str::<Value>(body).ok();
    let message = parsed.as_ref().and_then(|value| value.pointer("/error/message").and_then(Value::as_str)).unwrap_or(body);
    message.chars().take(240).collect()
}

fn codex_probe_message(compatibility: &CodexCompatibility) -> String {
    match compatibility {
        CodexCompatibility::Full => "模型列表读取成功；Codex Responses 与 custom 工具探测通过。兼容性探测会产生最多 1 token 的极小请求。".into(),
        CodexCompatibility::FunctionToolsOnly => "模型列表读取成功；网关拒绝 custom 工具，程序将关闭自由格式补丁声明并使用 Codex 默认兼容工具。".into(),
        CodexCompatibility::ChatProxy => "模型列表与 Chat Completions function 工具探测通过；程序将自动启用仅监听本机的 Responses 兼容桥，Provider Deck 使用期间需要保持运行。".into(),
        CodexCompatibility::ResponsesUnsupported => "模型列表可读取，但网关不支持当前 Codex 必需的 Responses 工具协议，暂不能自动配置 Codex CLI。".into(),
        CodexCompatibility::Unknown => "模型列表读取成功；兼容性探测因网络或限流未完成，程序将关闭自由格式补丁声明并采用保守模式。".into(),
        CodexCompatibility::NotApplicable => "模型列表读取成功。".into(),
    }
}

async fn probe_openai(client: &Client, base: &str, key: &str) -> Result<(String, Vec<ModelInfo>, f64), (String, String)> {
    let target = endpoint(base, "models", "/v1").map_err(|e| (base.into(), e.to_string()))?;
    let response = client.get(&target).header(AUTHORIZATION, format!("Bearer {key}")).header(CONTENT_TYPE, "application/json").send().await.map_err(|e| (target.clone(), classify_reqwest(&e)))?;
    let status = response.status();
    let headers = response.headers().clone();
    let body: Value = response.json().await.map_err(|e| (target.clone(), format!("响应不是有效 JSON：{e}")))?;
    if !status.is_success() { return Err((target, classify_status(status, &body))); }
    let data = body.get("data").and_then(Value::as_array).ok_or_else(|| (target.clone(), "响应缺少 data 模型数组".into()))?;
    let models = data.iter().filter_map(|item| item.get("id")?.as_str().map(|id| ModelInfo { id: id.into(), display_name: item.get("display_name").and_then(Value::as_str).unwrap_or(id).into(), provider: item.get("owned_by").and_then(Value::as_str).map(str::to_owned), protocol: ProtocolKind::Openai, source: "server".into(), capabilities: Vec::new(), context_window: context_window(item), parameter_count_billions: parameter_count_billions(item) })).collect();
    let confidence = if headers.contains_key("openai-version") || body.get("object").and_then(Value::as_str) == Some("list") { 0.98 } else { 0.88 };
    Ok((target, models, confidence))
}

async fn probe_anthropic(client: &Client, base: &str, key: &str) -> Result<(String, Vec<ModelInfo>, f64), (String, String)> {
    let target = endpoint(base, "models", "/v1").map_err(|e| (base.into(), e.to_string()))?;
    let response = client.get(&target).header("x-api-key", key).header("anthropic-version", "2023-06-01").send().await.map_err(|e| (target.clone(), classify_reqwest(&e)))?;
    let status = response.status();
    let headers = response.headers().clone();
    let body: Value = response.json().await.map_err(|e| (target.clone(), format!("响应不是有效 JSON：{e}")))?;
    if !status.is_success() { return Err((target, classify_status(status, &body))); }
    let data = body.get("data").and_then(Value::as_array).ok_or_else(|| (target.clone(), "响应缺少 data 模型数组".into()))?;
    let models = data.iter().filter_map(|item| item.get("id")?.as_str().map(|id| ModelInfo { id: id.into(), display_name: item.get("display_name").and_then(Value::as_str).unwrap_or(id).into(), provider: Some("Anthropic-compatible".into()), protocol: ProtocolKind::Anthropic, source: "server".into(), capabilities: Vec::new(), context_window: context_window(item), parameter_count_billions: parameter_count_billions(item) })).collect();
    let confidence = if headers.keys().any(|name| name.as_str().starts_with("anthropic-")) || data.iter().any(|item| item.get("type").and_then(Value::as_str) == Some("model")) { 0.98 } else { 0.86 };
    Ok((target, models, confidence))
}

async fn probe_gemini(client: &Client, base: &str, key: &str) -> Result<(String, Vec<ModelInfo>, f64), (String, String)> {
    let target = endpoint(base, "models", "/v1beta").map_err(|e| (base.into(), e.to_string()))?;
    let response = client.get(&target).header("x-goog-api-key", key).send().await.map_err(|e| (target.clone(), classify_reqwest(&e)))?;
    let status = response.status();
    let body: Value = response.json().await.map_err(|e| (target.clone(), format!("响应不是有效 JSON：{e}")))?;
    if !status.is_success() { return Err((target, classify_status(status, &body))); }
    let data = body.get("models").and_then(Value::as_array).ok_or_else(|| (target.clone(), "响应缺少 models 数组".into()))?;
    let models = data.iter().filter_map(|item| item.get("name")?.as_str().map(|name| { let id = name.strip_prefix("models/").unwrap_or(name); ModelInfo { id: id.into(), display_name: item.get("displayName").and_then(Value::as_str).unwrap_or(id).into(), provider: Some("Gemini-compatible".into()), protocol: ProtocolKind::Gemini, source: "server".into(), capabilities: item.get("supportedGenerationMethods").and_then(Value::as_array).map(|v| v.iter().filter_map(Value::as_str).map(str::to_owned).collect()).unwrap_or_default(), context_window: context_window(item), parameter_count_billions: parameter_count_billions(item) } })).collect();
    Ok((target, models, 0.98))
}

fn classify_status(status: StatusCode, body: &Value) -> String {
    let detail = body.pointer("/error/message").and_then(Value::as_str).unwrap_or("");
    let summary = match status.as_u16() { 401 => "身份验证失败，请检查 API Key", 403 => "服务拒绝访问，请检查密钥权限或地区限制", 404 => "模型列表接口不存在", 429 => "请求过于频繁或额度受限", 500..=599 => "服务端暂时不可用", _ => "服务返回异常状态" };
    if detail.is_empty() { format!("{summary}（HTTP {}）", status.as_u16()) } else { format!("{summary}（HTTP {}）：{detail}", status.as_u16()) }
}

fn classify_reqwest(error: &reqwest::Error) -> String {
    if error.is_timeout() { "连接超时，请检查网络、代理和服务地址".into() }
    else if error.is_connect() { format!("无法连接服务，可能是 DNS、TLS、代理或端口问题：{error}") }
    else { format!("网络请求失败：{error}") }
}

fn user_facing_failure(detail: &str) -> String {
    if detail.contains("401") || detail.contains("身份验证") { "所有安全探测均未通过身份验证，请检查 API Key 或手动选择协议".into() }
    else if detail.contains("超时") { "连接超时，请检查网络、代理、Base URL 和服务状态".into() }
    else { format!("无法自动识别协议。可手动选择协议和模型。技术摘要：{detail}") }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{io::{Read, Write}, net::TcpListener, thread};

    fn mock_responses_server(responses: Vec<(u16, &'static str, &'static str)>) -> (String, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let handle = thread::spawn(move || {
            for (status, body, expected_tool) in responses {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = Vec::new();
                let mut chunk = [0_u8; 4096];
                loop {
                    let read = stream.read(&mut chunk).unwrap();
                    if read == 0 { break; }
                    request.extend_from_slice(&chunk[..read]);
                    let Some(header_end) = request.windows(4).position(|window| window == b"\r\n\r\n") else { continue; };
                    let headers = String::from_utf8_lossy(&request[..header_end]);
                    let content_length = headers.lines().find_map(|line| {
                        let (name, value) = line.split_once(':')?;
                        name.eq_ignore_ascii_case("content-length").then(|| value.trim().parse::<usize>().ok()).flatten()
                    }).unwrap_or(0);
                    if request.len() >= header_end + 4 + content_length { break; }
                }
                let request = String::from_utf8_lossy(&request);
                assert!(request.contains(expected_tool), "request did not contain expected tool: {request}");
                let reason = if status == 200 { "OK" } else { "Error" };
                write!(stream, "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
            }
        });
        (format!("http://{address}/v1/responses"), handle)
    }
    #[test]
    fn normalizes_urls_without_overwriting_http() {
        assert_eq!(normalize_base_url(" api.example.com/v1/ ").unwrap(), "https://api.example.com/v1");
        assert_eq!(normalize_base_url("http://127.0.0.1:11434/v1/").unwrap(), "http://127.0.0.1:11434/v1");
    }
    #[test]
    fn preserves_known_prefix() {
        assert_eq!(endpoint("https://api.example.com/v1", "models", "/v1").unwrap(), "https://api.example.com/v1/models");
    }
    #[test]
    fn reads_common_context_window_fields() {
        assert_eq!(context_window(&serde_json::json!({ "context_window": 1_000_000 })), Some(1_000_000));
        assert_eq!(context_window(&serde_json::json!({ "contextLength": 128_000 })), Some(128_000));
        assert_eq!(context_window(&serde_json::json!({ "limits": { "context_window": "256000" } })), Some(256_000));
    }
    #[test]
    fn reads_parameter_count_metadata() {
        assert_eq!(parameter_count_billions(&serde_json::json!({ "parameter_count_billions": 70 })), Some(70.0));
        assert_eq!(parameter_count_billions(&serde_json::json!({ "metadata": { "parameter_count_billions": "32B" } })), Some(32.0));
    }

    #[tokio::test]
    async fn falls_back_to_function_tools_after_custom_schema_error() {
        let (target, server) = mock_responses_server(vec![
            (400, r#"{"error":{"message":"unknown variant `custom`"}}"#, r#""type":"custom""#),
            (200, r#"{"id":"resp_test","output":[]}"#, r#""type":"function""#),
        ]);
        let client = Client::builder().timeout(Duration::from_secs(5)).build().unwrap();
        let (compatibility, _) = probe_responses_tools(&client, &target, "test-key", "test-model").await;
        server.join().unwrap();
        assert_eq!(compatibility, CodexCompatibility::FunctionToolsOnly);
    }

    #[tokio::test]
    async fn marks_missing_responses_endpoint_as_unsupported() {
        let (target, server) = mock_responses_server(vec![
            (404, r#"{"error":{"message":"not found"}}"#, r#""type":"custom""#),
        ]);
        let client = Client::builder().timeout(Duration::from_secs(5)).build().unwrap();
        let (compatibility, _) = probe_responses_tools(&client, &target, "test-key", "test-model").await;
        server.join().unwrap();
        assert_eq!(compatibility, CodexCompatibility::ResponsesUnsupported);
    }

    #[tokio::test]
    async fn accepts_chat_function_tools_for_local_proxy() {
        let (target, server) = mock_responses_server(vec![
            (200, r#"{"id":"chatcmpl_test","choices":[{"message":{"role":"assistant","content":"OK"}}]}"#, r#""type":"function""#),
        ]);
        let client = Client::builder().timeout(Duration::from_secs(5)).build().unwrap();
        let (supported, detail) = probe_chat_tools(&client, &target, "test-key", "dynamic-model").await;
        server.join().unwrap();
        assert!(supported);
        assert!(detail.contains("本机 Responses 兼容桥"));
    }
}
