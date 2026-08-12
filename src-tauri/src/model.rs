use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::reasoning_capability::ReasoningCapability;
use crate::reasoning_selection::ReasoningSelection;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ProtocolKind {
    Openai,
    Anthropic,
    Gemini,
    AzureOpenai,
    Custom,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ClaudeModelProfile {
    Sonnet,
    Opus,
    Haiku,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum CodexCompatibility {
    Full,
    FunctionToolsOnly,
    ChatProxy,
    ResponsesUnsupported,
    Unknown,
    NotApplicable,
}

impl Default for CodexCompatibility {
    fn default() -> Self { Self::Unknown }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeModelMappings {
    pub sonnet: Option<String>,
    pub opus: Option<String>,
    pub haiku: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelInfo {
    pub id: String,
    pub display_name: String,
    pub provider: Option<String>,
    pub protocol: ProtocolKind,
    pub source: String,
    pub capabilities: Vec<String>,
    pub context_window: Option<u64>,
    #[serde(default)]
    pub parameter_count_billions: Option<f64>,
    #[serde(default)]
    pub reasoning: Option<ReasoningCapability>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ReasoningLevel {
    Low,
    Medium,
    High,
}

impl Default for ReasoningLevel {
    fn default() -> Self { Self::High }
}

impl ReasoningLevel {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Provider {
    pub id: String,
    pub name: String,
    pub base_url: String,
    pub protocol: ProtocolKind,
    pub enabled: bool,
    pub is_current: bool,
    pub default_model: Option<String>,
    #[serde(default)]
    pub claude_model_profile: Option<ClaudeModelProfile>,
    #[serde(default)]
    pub claude_extended_context: bool,
    #[serde(default)]
    pub claude_model_mappings: ClaudeModelMappings,
    #[serde(default)]
    pub codex_compatibility: CodexCompatibility,
    #[serde(default)]
    pub codex_probe_model: Option<String>,
    #[serde(default)]
    pub codex_probe_detail: Option<String>,
    /// 用户对本 Provider 各模型的推理档位选择。
    ///
    /// 挂在 Provider 而不是 ModelInfo：models 在 save/reprobe/refresh 三处都是整体替换，
    /// 把用户意图放进去每次刷新都要靠迁移逻辑救一次。挂这里只需按 model_id 剪枝。
    /// 同时守住"ModelInfo 装发现到的事实，Provider 装用户的意图"这条边界。
    #[serde(default)]
    pub reasoning_selections: Vec<ReasoningSelection>,
    pub models: Vec<ModelInfo>,
    pub connection_state: String,
    pub confidence: Option<f64>,
    pub last_checked_at: Option<String>,
    pub applied_clients: Vec<String>,
    pub error_summary: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderDraft {
    pub id: Option<String>,
    pub name: String,
    pub base_url: String,
    pub api_key: String,
    pub protocol_hint: Option<ProtocolKind>,
    pub timeout_seconds: u64,
    pub azure_api_version: Option<String>,
    #[serde(default)]
    pub default_model: Option<String>,
    #[serde(default)]
    pub claude_model_profile: Option<ClaudeModelProfile>,
    #[serde(default)]
    pub claude_extended_context: bool,
    #[serde(default)]
    pub claude_model_mappings: ClaudeModelMappings,
    /// 前端提交的推理档位选择。可以只带当前正在编辑的模型，
    /// 合并时草稿优先、未提到的模型保留原值（见 reasoning_selection::merge_drafted）。
    #[serde(default)]
    pub reasoning_selections: Vec<ReasoningSelection>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProbeResult {
    pub normalized_base_url: String,
    pub protocol: ProtocolKind,
    pub confidence: f64,
    pub models: Vec<ModelInfo>,
    #[serde(default)]
    pub codex_compatibility: CodexCompatibility,
    #[serde(default)]
    pub codex_probe_model: Option<String>,
    #[serde(default)]
    pub codex_probe_detail: Option<String>,
    pub checked_endpoints: Vec<String>,
    pub user_message: String,
    pub technical_detail: Option<String>,
    /// 推理能力发现的说明（限流、端点不可达、需要真实请求才能确证等）。
    /// 与 codexProbeDetail 同级，供前端展示；能力本体挂在 ModelInfo.reasoning 上。
    #[serde(default)]
    pub reasoning_note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderTestCheck {
    pub id: String,
    pub label: String,
    pub status: String,
    pub detail: String,
    pub latency_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderTestReport {
    pub provider_id: String,
    pub model: Option<String>,
    pub total_latency_ms: u64,
    pub checks: Vec<ProviderTestCheck>,
    pub reply_preview: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientDescriptor {
    pub id: String,
    pub name: String,
    pub platforms: Vec<String>,
    pub protocols: Vec<ProtocolKind>,
    pub installed: bool,
    pub detected_path: Option<String>,
    pub config_path: Option<String>,
    pub support: String,
    pub auto_config: bool,
    pub requires_restart: bool,
    pub guidance: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigChange {
    pub client_id: String,
    pub client_name: String,
    pub target_path: Option<String>,
    pub support: String,
    pub can_write: bool,
    pub format: String,
    pub before_preview: String,
    pub after_preview: String,
    pub warnings: Vec<String>,
    pub expected_hash: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyResult {
    pub client_id: String,
    pub success: bool,
    pub backup_id: Option<String>,
    pub message: String,
    pub restart_required: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupRecord {
    pub id: String,
    pub client_id: String,
    pub target_path: String,
    pub backup_path: String,
    pub created_at: String,
    pub size: u64,
    #[serde(default)]
    pub original_exists: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    pub timeout_seconds: u64,
    pub proxy_url: String,
    pub allow_self_signed_certificates: bool,
    pub generate_only: bool,
    pub clear_clipboard_seconds: u64,
    pub locale: String,
    #[serde(default)]
    pub local_proxy_port: Option<u16>,
    #[serde(default)]
    pub auto_reasoning_mode: bool,
    #[serde(default)]
    pub manual_reasoning_level: ReasoningLevel,
    #[serde(default)]
    pub effective_reasoning_level: ReasoningLevel,
    #[serde(default)]
    pub reasoning_match_message: Option<String>,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            timeout_seconds: 10,
            proxy_url: String::new(),
            allow_self_signed_certificates: false,
            generate_only: true,
            clear_clipboard_seconds: 30,
            locale: "zh-CN".into(),
            local_proxy_port: None,
            auto_reasoning_mode: false,
            manual_reasoning_level: ReasoningLevel::High,
            effective_reasoning_level: ReasoningLevel::High,
            reasoning_match_message: None,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersistedState {
    pub providers: Vec<Provider>,
    pub backups: Vec<BackupRecord>,
    pub settings: AppSettings,
}
