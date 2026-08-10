use std::{collections::HashMap, convert::Infallible, net::{Ipv4Addr, SocketAddr}, sync::{Arc, RwLock}};

use async_stream::stream;
use axum::{
    body::Body,
    extract::{ConnectInfo, DefaultBodyLimit, Path, State},
    http::{header, HeaderMap, Response, StatusCode},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use bytes::Bytes;
use eventsource_stream::Eventsource;
use futures_util::StreamExt;
use reqwest::Client;
use serde_json::{json, Value};
use url::Url;

use crate::{
    activity,
    chat_store::ChatStore,
    error::{AppError, AppResult},
    model::{AppSettings, Provider},
    responses_chat::{chat_to_response, responses_to_chat, StreamAdapter},
};

#[derive(Clone)]
struct ProxyRoute {
    upstream_endpoint: String,
    api_key: String,
    local_token: String,
    client: Client,
    reasoning_effort: String,
}

struct ProxyState {
    routes: RwLock<HashMap<String, ProxyRoute>>,
    chats: ChatStore,
}

pub struct LocalProxy {
    address: SocketAddr,
    state: Arc<ProxyState>,
}

impl LocalProxy {
    pub fn start(preferred_port: Option<u16>, chats: ChatStore) -> AppResult<Self> {
        let bind = |port| std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, port));
        let listener = match preferred_port.filter(|port| *port > 0) {
            Some(port) => bind(port).or_else(|_| bind(0))?,
            None => bind(0)?,
        };
        listener.set_nonblocking(true)?;
        let address = listener.local_addr()?;
        let state = Arc::new(ProxyState { routes: RwLock::new(HashMap::new()), chats });
        let server_state = Arc::clone(&state);
        tauri::async_runtime::spawn(async move {
            let Ok(listener) = tokio::net::TcpListener::from_std(listener) else { return; };
            let app = Router::new()
                .route("/health", get(health))
                .route("/providers/{provider_id}/v1/responses", post(handle_responses))
                .layer(DefaultBodyLimit::max(64 * 1024 * 1024))
                .with_state(server_state);
            let _ = axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>()).await;
        });
        Ok(Self { address, state })
    }

    pub fn port(&self) -> u16 { self.address.port() }

    pub fn provider_base_url(&self, provider_id: &str) -> String {
        format!("http://127.0.0.1:{}/providers/{provider_id}/v1", self.port())
    }

    pub fn register(&self, provider: &Provider, api_key: &str, local_token: &str, settings: &AppSettings) -> AppResult<()> {
        let upstream_endpoint = chat_endpoint(&provider.base_url)?;
        let mut builder = Client::builder()
            .timeout(std::time::Duration::from_secs(settings.timeout_seconds.clamp(3, 120)))
            .danger_accept_invalid_certs(settings.allow_self_signed_certificates);
        if !settings.proxy_url.trim().is_empty() {
            builder = builder.proxy(reqwest::Proxy::all(settings.proxy_url.trim()).map_err(|error| AppError::InvalidInput(format!("代理地址无效：{error}")))?);
        }
        let route = ProxyRoute {
            upstream_endpoint,
            api_key: api_key.to_string(),
            local_token: local_token.to_string(),
            client: builder.build().map_err(|error| AppError::Network(error.to_string()))?,
            reasoning_effort: settings.effective_reasoning_level.as_str().into(),
        };
        self.state.routes.write().map_err(|_| AppError::Config("本地代理路由锁已损坏".into()))?.insert(provider.id.clone(), route);
        Ok(())
    }

    pub fn unregister(&self, provider_id: &str) {
        if let Ok(mut routes) = self.state.routes.write() { routes.remove(provider_id); }
    }
}

fn chat_endpoint(base: &str) -> AppResult<String> {
    let mut url = Url::parse(base).map_err(|_| AppError::InvalidInput("Base URL 格式无效".into()))?;
    let path = url.path().trim_end_matches('/');
    let next = if path.ends_with("/v1") { format!("{path}/chat/completions") } else { format!("{path}/v1/chat/completions") };
    url.set_path(&next.replace("//", "/"));
    Ok(url.to_string())
}

async fn health(ConnectInfo(peer): ConnectInfo<SocketAddr>) -> impl IntoResponse {
    if !peer.ip().is_loopback() { return (StatusCode::FORBIDDEN, Json(json!({ "error": "loopback only" }))); }
    (StatusCode::OK, Json(json!({ "status": "ok" })))
}

fn json_error(status: StatusCode, message: impl Into<String>) -> Response<Body> {
    let body = json!({ "error": { "message": message.into(), "type": "provider_deck_proxy_error", "code": status.as_u16() } });
    Response::builder().status(status).header(header::CONTENT_TYPE, "application/json").body(Body::from(body.to_string())).unwrap()
}

fn bearer(headers: &HeaderMap) -> Option<&str> {
    headers.get(header::AUTHORIZATION)?.to_str().ok()?.strip_prefix("Bearer ")
}

async fn handle_responses(
    State(state): State<Arc<ProxyState>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Path(provider_id): Path<String>,
    headers: HeaderMap,
    Json(mut payload): Json<Value>,
) -> Response<Body> {
    if !peer.ip().is_loopback() { return json_error(StatusCode::FORBIDDEN, "本地代理只接受环回连接"); }
    let route = match state.routes.read().ok().and_then(|routes| routes.get(&provider_id).cloned()) {
        Some(route) => route,
        None => return json_error(StatusCode::NOT_FOUND, "Provider 代理路由不存在，请保持 Provider Deck 运行并重新应用配置"),
    };
    if bearer(&headers) != Some(route.local_token.as_str()) { return json_error(StatusCode::UNAUTHORIZED, "本地代理令牌无效"); }

    if let Some(object) = payload.as_object_mut() {
        let reasoning = object.entry("reasoning").or_insert_with(|| json!({}));
        if let Some(reasoning) = reasoning.as_object_mut() {
            reasoning.insert("effort".into(), Value::String(route.reasoning_effort.clone()));
        }
    }
    let previous_messages = payload.get("previous_response_id").and_then(Value::as_str).and_then(|id| state.chats.get(id));
    if payload.get("previous_response_id").is_some() && previous_messages.is_none() {
        return json_error(StatusCode::CONFLICT, "previous_response_id 不属于当前代理进程；请开启新会话");
    }
    let converted = match responses_to_chat(&payload, previous_messages.as_deref()) {
        Ok(converted) => converted,
        Err(error) => return json_error(StatusCode::UNPROCESSABLE_ENTITY, error.to_string()),
    };
    let chat_messages = converted.body.get("messages").and_then(Value::as_array).cloned().unwrap_or_default();
    let upstream = match route.client.post(&route.upstream_endpoint)
        .bearer_auth(&route.api_key)
        .header(header::CONTENT_TYPE.as_str(), "application/json")
        .header("x-provider-deck-proxy", "responses-chat")
        .json(&converted.body)
        .send().await {
            Ok(response) => response,
            Err(error) => return json_error(StatusCode::BAD_GATEWAY, format!("连接 Chat 后端失败：{error}")),
        };
    if !upstream.status().is_success() { return passthrough_error(upstream).await; }

    let warning_count = converted.warnings.len();
    if converted.stream {
        stream_response(
            upstream,
            converted.body.get("model").and_then(Value::as_str).unwrap_or_default().to_string(),
            converted.tools,
            Arc::clone(&state),
            chat_messages,
            warning_count,
        )
    } else {
        let chat: Value = match upstream.json().await {
            Ok(value) => value,
            Err(error) => return json_error(StatusCode::BAD_GATEWAY, format!("Chat 后端响应不是有效 JSON：{error}")),
        };
        let response = match chat_to_response(&chat, &converted.tools) {
            Ok(value) => value,
            Err(error) => return json_error(StatusCode::BAD_GATEWAY, error.to_string()),
        };
        if let Some(response_id) = response.get("id").and_then(Value::as_str) {
            let mut conversation = chat_messages;
            if let Some(message) = chat.pointer("/choices/0/message") { conversation.push(message.clone()); }
            if let Err(error) = state.chats.record(response_id.to_string(), conversation) {
                activity::record("chat_cache_write", &error.to_string(), false);
            }
        }
        Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "application/json")
            .header("x-provider-deck-degraded-features", warning_count.to_string())
            .body(Body::from(response.to_string())).unwrap()
    }
}

async fn passthrough_error(upstream: reqwest::Response) -> Response<Body> {
    let status = StatusCode::from_u16(upstream.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    let content_type = upstream.headers().get(reqwest::header::CONTENT_TYPE).cloned();
    let retry_after = upstream.headers().get(reqwest::header::RETRY_AFTER).cloned();
    let bytes = upstream.bytes().await.unwrap_or_default();
    let mut builder = Response::builder().status(status);
    if let Some(value) = content_type { builder = builder.header(header::CONTENT_TYPE, value); }
    if let Some(value) = retry_after { builder = builder.header(header::RETRY_AFTER, value); }
    builder.body(Body::from(bytes)).unwrap()
}

fn stream_response(
    upstream: reqwest::Response,
    model: String,
    tools: crate::responses_chat::ToolMap,
    state: Arc<ProxyState>,
    chat_messages: Vec<Value>,
    warning_count: usize,
) -> Response<Body> {
    let mut upstream_events = upstream.bytes_stream().eventsource();
    let output = stream! {
        let mut adapter = StreamAdapter::new(model, tools);
        for event in adapter.start() { yield Ok::<Bytes, Infallible>(Bytes::from(event)); }
        while let Some(event) = upstream_events.next().await {
            match event {
                Ok(event) if event.data == "[DONE]" => break,
                Ok(event) => match serde_json::from_str::<Value>(&event.data) {
                    Ok(chunk) => for converted in adapter.push_chat_chunk(&chunk) { yield Ok(Bytes::from(converted)); },
                    Err(error) => {
                        let failed = json!({ "type": "response.failed", "response": { "status": "failed", "error": { "message": format!("无法解析 Chat SSE：{error}") } } });
                        yield Ok(Bytes::from(format!("event: response.failed\ndata: {failed}\n\n")));
                        return;
                    }
                },
                Err(error) => {
                    let failed = json!({ "type": "response.failed", "response": { "status": "failed", "error": { "message": format!("Chat SSE 中断：{error}") } } });
                    yield Ok(Bytes::from(format!("event: response.failed\ndata: {failed}\n\n")));
                    return;
                }
            }
        }
        let (response_id, assistant_message) = adapter.conversation_snapshot();
        for event in adapter.finish() { yield Ok(Bytes::from(event)); }
        let mut conversation = chat_messages;
        conversation.push(assistant_message);
        if let Err(error) = state.chats.record(response_id, conversation) {
            activity::record("chat_cache_write", &error.to_string(), false);
        }
    };
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/event-stream")
        .header(header::CACHE_CONTROL, "no-cache")
        .header(header::CONNECTION, "keep-alive")
        .header("x-provider-deck-degraded-features", warning_count.to_string())
        .body(Body::from_stream(output)).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{io::{Read, Write}, net::TcpListener, thread};

    fn test_provider(base_url: String) -> Provider {
        Provider {
            id: "proxy-test-provider".into(), name: "Proxy Test".into(), base_url,
            protocol: crate::model::ProtocolKind::Openai, enabled: true, is_current: true,
            default_model: Some("dynamic-model".into()), claude_model_profile: None,
            claude_extended_context: false, claude_model_mappings: Default::default(),
            codex_compatibility: crate::model::CodexCompatibility::ChatProxy,
            codex_probe_model: Some("dynamic-model".into()), codex_probe_detail: None,
            models: vec![], connection_state: "connected".into(), confidence: Some(1.0),
            last_checked_at: None, applied_clients: vec![], error_summary: None,
        }
    }

    fn spawn_upstream(status: u16, body: &'static str) -> (String, thread::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let handle = thread::spawn(move || {
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
            let request_text = String::from_utf8_lossy(&request).to_string();
            let reason = if status == 200 { "OK" } else { "Error" };
            let response = format!("HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
            stream.write_all(response.as_bytes()).unwrap();
            request_text
        });
        (format!("http://{address}/v1"), handle)
    }

    #[test]
    fn appends_chat_endpoint_without_duplicate_v1() {
        assert_eq!(chat_endpoint("https://example.com/v1").unwrap(), "https://example.com/v1/chat/completions");
        assert_eq!(chat_endpoint("https://example.com/prefix").unwrap(), "https://example.com/prefix/v1/chat/completions");
    }

    #[tokio::test]
    async fn translates_custom_tool_and_preserves_upstream_auth_boundary() {
        let (upstream_base, server) = spawn_upstream(200, r#"{"id":"chatcmpl-test","model":"dynamic-model","choices":[{"message":{"role":"assistant","content":null,"tool_calls":[{"id":"call_1","type":"function","function":{"name":"pd_custom_48458d4adc_apply_patch","arguments":"{\"input\":\"patch\"}"}}]},"finish_reason":"tool_calls"}]}"#);
        let proxy = LocalProxy::start(None, ChatStore::load().unwrap()).unwrap();
        let provider = test_provider(upstream_base);
        proxy.register(&provider, "upstream-secret", "local-secret", &AppSettings::default()).unwrap();
        let client = Client::new();
        let response = client.post(format!("{}/responses", proxy.provider_base_url(&provider.id)))
            .bearer_auth("local-secret")
            .json(&json!({ "model": "dynamic-model", "input": "apply", "tools": [{ "type": "custom", "name": "apply_patch", "format": { "type": "text" } }, { "type": "namespace", "name": "ignored" }] }))
            .send().await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let converted: Value = response.json().await.unwrap();
        assert_eq!(converted["output"][0]["type"], "custom_tool_call");
        assert_eq!(converted["output"][0]["name"], "apply_patch");
        let request = server.join().unwrap();
        assert!(request.contains("POST /v1/chat/completions"));
        assert!(request.contains("Authorization: Bearer upstream-secret"));
        assert!(!request.contains("\"type\":\"custom\""));
        assert!(!request.contains("\"type\":\"namespace\""));
    }

    #[tokio::test]
    async fn passes_upstream_error_status_and_body_to_codex() {
        let (upstream_base, server) = spawn_upstream(401, r#"{"error":{"message":"upstream denied"}}"#);
        let proxy = LocalProxy::start(None, ChatStore::load().unwrap()).unwrap();
        let provider = test_provider(upstream_base);
        proxy.register(&provider, "upstream-secret", "local-secret", &AppSettings::default()).unwrap();
        let response = Client::new().post(format!("{}/responses", proxy.provider_base_url(&provider.id)))
            .bearer_auth("local-secret")
            .json(&json!({ "model": "dynamic-model", "input": "hello" }))
            .send().await.unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(response.text().await.unwrap(), r#"{"error":{"message":"upstream denied"}}"#);
        server.join().unwrap();
    }

    #[tokio::test]
    async fn translates_chat_sse_to_responses_events() {
        let upstream_sse = concat!(
            "data: {\"id\":\"chatcmpl-stream\",\"model\":\"dynamic-model\",\"choices\":[{\"delta\":{\"role\":\"assistant\",\"content\":\"hello\"},\"finish_reason\":null}]}\n\n",
            "data: {\"id\":\"chatcmpl-stream\",\"model\":\"dynamic-model\",\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
            "data: [DONE]\n\n"
        );
        let (upstream_base, server) = spawn_upstream(200, upstream_sse);
        let proxy = LocalProxy::start(None, ChatStore::load().unwrap()).unwrap();
        let provider = test_provider(upstream_base);
        proxy.register(&provider, "upstream-secret", "local-secret", &AppSettings::default()).unwrap();
        let response = Client::new().post(format!("{}/responses", proxy.provider_base_url(&provider.id)))
            .bearer_auth("local-secret")
            .json(&json!({ "model": "dynamic-model", "input": "hello", "stream": true }))
            .send().await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers().get(header::CONTENT_TYPE).and_then(|value| value.to_str().ok()), Some("text/event-stream"));
        let body = response.text().await.unwrap();
        assert!(body.contains("event: response.output_text.delta"));
        assert!(body.contains("event: response.completed"));
        server.join().unwrap();
    }
}
