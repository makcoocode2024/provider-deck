use std::{fs, path::{Path, PathBuf}, str::FromStr};
use chrono::Utc;
use directories::ProjectDirs;
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use toml_edit::{value, DocumentMut, Item, Table};
use url::Url;
use uuid::Uuid;
use crate::{clients, error::{AppError, AppResult}, model::{AppSettings, BackupRecord, ClaudeModelMappings, ClaudeModelProfile, CodexCompatibility, ConfigChange, ProtocolKind, Provider}, storage::atomic_replace};
use crate::reasoning_adapters::adapter_for;
use crate::reasoning_capability::{ReasoningBinding, ReasoningCapability, ReasoningControl, ReasoningSupport};
use crate::reasoning_selection;

/// Codex 能表达的推理配置。
///
/// `config.toml` 的 `model_reasoning_effort` 只是一个字符串，而 Adapter 产出的是协议原生
/// JSON——OpenAI 是 `{"reasoning":{"effort":"medium"}}`，Anthropic 是
/// `{"thinking":{"budget_tokens":8192}}`，Gemini 是 `{"generationConfig":{...}}`。
/// 后两者在 Codex 里根本无处安放，所以这一层只做一件事：把能落进那个字符串的取出来，
/// 落不进的**省略**并留下原因，绝不把 8192 硬塞成 `model_reasoning_effort = "8192"`。
#[derive(Debug, Default, Clone)]
struct CodexReasoning {
    /// 写进 `model_reasoning_effort` / `default_reasoning_level` 的取值。
    effort: Option<String>,
    /// 写进 `supported_reasoning_levels` 的 (effort, description) 列表，
    /// 全部由 `capability.control` 派生，没有任何内置档位表。
    supported: Vec<(String, String)>,
    /// 中文说明，用于 preview 警告与调试。
    reason: String,
}

impl CodexReasoning {
    /// 该模型没有任何可写的推理信息：catalog 里连 reasoning 相关键都不出现。
    fn is_empty(&self) -> bool {
        self.effort.is_none() && self.supported.is_empty()
    }
}

/// `(Provider, model_id)` → Codex 可写的推理配置。
///
/// 链路：`ModelInfo.reasoning` + `Provider.reasoning_selections` →
/// [`reasoning_selection::resolve_binding`] → [`ReasoningAdapter::apply_reasoning_config`] →
/// 本函数抽取 Codex 字段。旧的全局 `effective_reasoning_level` 只作为 legacy fallback
/// 传给 resolver，不再是主来源。
fn codex_reasoning(provider: &Provider, model_id: &str, settings: &AppSettings) -> CodexReasoning {
    let capability = provider.models.iter()
        .find(|item| item.id == model_id)
        .and_then(|item| item.reasoning.as_ref());
    let selection = reasoning_selection::selection_for(&provider.reasoning_selections, model_id);
    let resolved = reasoning_selection::resolve_binding(
        capability,
        selection,
        Some(settings.effective_reasoning_level),
    );

    // 档位清单直接来自发现结果。服务端将来新增 xhigh / ultra / 任何新成员，
    // 这里自动出现，不需要改代码——所以本函数里不允许出现任何档位字面量。
    let supported = capability
        .filter(|item| item.support == ReasoningSupport::Supported)
        .map(declared_effort_levels)
        .unwrap_or_default();

    // 无证据 ≠ 反证。这两种"省略"必须分开处理：
    //
    // - 能力缺失 / Unknown：什么都没探到。resolve_binding 对请求参数返回 Omitted 是对的
    //   （发一个网关不认的参数会 400），但 config.toml 是用户已经在用的配置文件，
    //   升级一次版本就把 model_reasoning_effort 抹掉属于让旧配置失效。这里沿用旧的全局
    //   档位——它不与任何已发现的事实冲突，只是维持现状。
    // - Unsupported：探到了"不支持"这个事实。此时写任何档位都是已知错误，保持沉默。
    let legacy_kept = || {
        let legacy = settings.effective_reasoning_level.as_str().to_owned();
        CodexReasoning {
            effort: Some(legacy.clone()),
            // 没有任何已声明的成员，就不编造清单。Codex 侧表现为该模型不提供档位选择，
            // 但 model_reasoning_effort 仍然生效。
            supported: supported.clone(),
            reason: format!("{}；沿用旧的全局档位 \"{legacy}\" 以保持既有配置不变", resolved.reason),
        }
    };
    let capability = match capability {
        None => return legacy_kept(),
        Some(item) if matches!(item.support, ReasoningSupport::Unknown) => return legacy_kept(),
        Some(item) => item,
    };

    if resolved.is_omitted() {
        return CodexReasoning { effort: None, supported, reason: resolved.reason };
    }

    // 协议原生映射仍然由 Adapter 负责，config.rs 不重新实现一遍。
    // tier 为 None 只发生在用户钉死了 explicit_binding 的情况，此时绕过档位语义，
    // 直接读绑定本身。
    let effort = match resolved.tier {
        Some(tier) => adapter_for(provider.protocol)
            .apply_reasoning_config(capability, tier)
            .as_ref()
            .and_then(|native| native.pointer("/reasoning/effort"))
            .and_then(Value::as_str)
            .map(str::to_owned),
        None => match &resolved.binding {
            ReasoningBinding::Effort { value } => Some(value.clone()),
            _ => None,
        },
    };

    // 只接受服务端**声明过**的成员。这不是白名单，而是"取值必须有出处"：
    // Adapter 会把 Disabled 合成为 effort="off"，而 off 未必是这个网关的合法成员，
    // 写进 config.toml 会让 Codex 启动即报错。
    let effort = effort.filter(|value| {
        matches!(&capability.control, ReasoningControl::EffortEnum { values } if values.iter().any(|member| member == value))
    });

    let reason = match &effort {
        Some(value) => format!("{}，写入 model_reasoning_effort = \"{value}\"", resolved.reason),
        None => format!(
            "{}；该绑定无法用 Codex 的 model_reasoning_effort 表达，已省略该字段（budget binding cannot be represented by Codex model_reasoning_effort）",
            resolved.reason
        ),
    };
    CodexReasoning { effort, supported, reason }
}

/// 由 `capability.control` 派生 Codex 的 `supported_reasoning_levels`。
///
/// 描述文本取自档位表里同绑定的那一档标签；`tiers` 是 `control.values` 的策展视图
/// （成员多于 4 个时只挑代表），落选成员拿不到标签，给一句中性描述而不是编造语义。
fn declared_effort_levels(capability: &ReasoningCapability) -> Vec<(String, String)> {
    let ReasoningControl::EffortEnum { values } = &capability.control else { return Vec::new(); };
    values.iter().map(|value| {
        let description = capability.tiers.iter()
            .find(|option| matches!(&option.binding, ReasoningBinding::Effort { value: bound } if bound == value))
            .map(|option| option.label.clone())
            .unwrap_or_else(|| format!("服务端声明的推理档位 {value}"));
        (value.clone(), description)
    }).collect()
}

fn slug(provider: &Provider) -> String {
    let filtered: String = provider.name.to_lowercase().chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect();
    let compact = filtered.split('-').filter(|part| !part.is_empty()).collect::<Vec<_>>().join("-");
    if compact.is_empty() { format!("provider-{}", &provider.id[..8.min(provider.id.len())]) } else { compact }
}

pub fn file_hash(path: &Path) -> AppResult<String> {
    if !path.exists() { return Ok("missing".into()); }
    let mut hasher = Sha256::new();
    hasher.update(fs::read(path)?);
    Ok(hex::encode(hasher.finalize()))
}

fn read_text(path: &Path) -> AppResult<String> {
    if path.exists() { fs::read_to_string(path).map_err(Into::into) } else { Ok(String::new()) }
}

fn merge_codex(existing: &str, provider: &Provider, secret: &str, settings: &AppSettings) -> AppResult<String> {
    let mut doc = if existing.trim().is_empty() { DocumentMut::new() } else { DocumentMut::from_str(existing).map_err(|e| AppError::Config(format!("Codex TOML 解析失败：{e}")))? };
    let id = slug(provider);
    doc["model_provider"] = value(id.clone());
    if let Some(model) = &provider.default_model { doc["model"] = value(model.clone()); }
    if !doc.as_table().contains_key("model_providers") { doc["model_providers"] = Item::Table(Table::new()); }
    doc["model_providers"][&id]["name"] = value(provider.name.clone());
    doc["model_providers"][&id]["base_url"] = value(provider.base_url.clone());
    doc["model_providers"][&id]["wire_api"] = value("responses");
    doc["model_providers"][&id]["requires_openai_auth"] = value(false);
    doc["model_providers"][&id]["experimental_bearer_token"] = value(secret);
    doc["model_catalog_json"] = value(codex_catalog_path()?.to_string_lossy().into_owned());
    if let Some(model) = &provider.default_model {
        let context_window = provider.models.iter().find(|item| &item.id == model).and_then(|item| item.context_window).unwrap_or(200_000);
        doc["model"] = value(model.clone());
        doc["model_context_window"] = value(context_window as i64);
        match codex_reasoning(provider, model, settings).effort {
            Some(effort) => doc["model_reasoning_effort"] = value(effort),
            // 主动删除而不是留着：merge 是就地合并，上一次写的旧档位若不删，
            // 会以一个此模型并不支持的取值继续生效。
            None => { doc.as_table_mut().remove("model_reasoning_effort"); }
        }
        doc["model_supports_reasoning_summaries"] = value(false);
        doc["model_reasoning_summary"] = value("none");
    }
    Ok(doc.to_string())
}

fn codex_catalog_path() -> AppResult<PathBuf> {
    let config_path = clients::config_path("codex-cli").ok_or_else(|| AppError::Config("无法确定 Codex 配置目录".into()))?;
    let parent = config_path.parent().ok_or_else(|| AppError::Config("Codex 配置路径缺少父目录".into()))?;
    Ok(parent.join("provider-deck-model-catalog.json"))
}

fn remove_legacy_function_patch_type(catalog: &mut Value) -> bool {
    let Some(models) = catalog.get_mut("models").and_then(Value::as_array_mut) else { return false; };
    let mut changed = false;
    for model in models {
        let Some(object) = model.as_object_mut() else { continue; };
        if object.get("apply_patch_tool_type").and_then(Value::as_str) == Some("function") {
            object.remove("apply_patch_tool_type");
            changed = true;
        }
    }
    changed
}

pub fn repair_legacy_codex_catalog() -> AppResult<bool> {
    let path = codex_catalog_path()?;
    if !path.exists() { return Ok(false); }
    let mut catalog: Value = serde_json::from_str(&fs::read_to_string(&path)?)
        .map_err(|error| AppError::Config(format!("Provider Deck 模型目录解析失败：{error}")))?;
    if !remove_legacy_function_patch_type(&mut catalog) { return Ok(false); }
    let bytes = serde_json::to_vec_pretty(&catalog).map_err(|error| AppError::Config(error.to_string()))?;
    atomic_replace(&path, &bytes)?;
    restrict_permissions(&path)?;
    Ok(true)
}

fn codex_catalog(provider: &Provider, settings: &AppSettings) -> AppResult<String> {
    let mut models = provider.models.clone();
    if let Some(default_model) = &provider.default_model {
        if !models.iter().any(|model| &model.id == default_model) {
            models.push(crate::model::ModelInfo {
                id: default_model.clone(),
                display_name: default_model.clone(),
                provider: None,
                protocol: provider.protocol.clone(),
                source: "manual".into(),
                capabilities: Vec::new(),
                context_window: None,
                parameter_count_billions: None,
                reasoning: None,
            });
        }
    }
    let entries = models.iter().enumerate().map(|(index, model)| {
        let context_window = model.context_window.unwrap_or(200_000);
        let mut entry = json!({
            "slug": model.id,
            "display_name": model.display_name,
            "description": format!("{} via Provider Deck", model.display_name),
            "shell_type": "shell_command",
            "visibility": "list",
            "supported_in_api": true,
            "priority": index,
            "additional_speed_tiers": [],
            "service_tiers": [],
            "availability_nux": null,
            "upgrade": null,
            "base_instructions": "You are Codex, a coding agent. Work carefully in the user's current workspace, follow the user's instructions, inspect existing code before editing, preserve unrelated changes, use available tools when needed, and verify completed work before reporting it.",
            "supports_reasoning_summaries": false,
            "default_reasoning_summary": "none",
            "support_verbosity": false,
            "default_verbosity": "medium",
            "web_search_tool_type": "text",
            "truncation_policy": { "mode": "tokens", "limit": 10_000 },
            "supports_parallel_tool_calls": true,
            "supports_image_detail_original": false,
            "context_window": context_window,
            "max_context_window": context_window,
            "effective_context_window_percent": 95,
            "experimental_supported_tools": [],
            "input_modalities": ["text"],
            "supports_search_tool": false,
            "use_responses_lite": false
        });
        if matches!(provider.codex_compatibility, CodexCompatibility::Full | CodexCompatibility::ChatProxy) {
            entry["apply_patch_tool_type"] = Value::String("freeform".into());
        }
        // 推理档位按**模型**逐个解析：同一个 Provider 下 model-a 可以是 high、
        // model-b 可以完全不支持推理。之前这里对所有模型写同一份全局值 + 同一张硬编码表。
        let reasoning = codex_reasoning(provider, &model.id, settings);
        if !reasoning.is_empty() {
            if let Some(effort) = &reasoning.effort {
                entry["default_reasoning_level"] = Value::String(effort.clone());
            }
            if !reasoning.supported.is_empty() {
                entry["supported_reasoning_levels"] = Value::Array(
                    reasoning.supported.iter()
                        .map(|(effort, description)| json!({ "effort": effort, "description": description }))
                        .collect(),
                );
            }
        }
        entry
    }).collect::<Vec<_>>();
    serde_json::to_string_pretty(&json!({ "models": entries })).map_err(|error| AppError::Config(error.to_string()))
}

fn merge_claude(existing: &str, provider: &Provider, secret: &str) -> AppResult<String> {
    let mut root: Value = if existing.trim().is_empty() { json!({}) } else { serde_json::from_str(existing).map_err(|e| AppError::Config(format!("Claude settings.json 解析失败：{e}")))? };
    let object = root.as_object_mut().ok_or_else(|| AppError::Config("Claude settings.json 顶层必须是对象".into()))?;
    let env = object.entry("env").or_insert_with(|| json!({})).as_object_mut().ok_or_else(|| AppError::Config("Claude env 字段必须是对象".into()))?;
    env.insert("ANTHROPIC_BASE_URL".into(), Value::String(anthropic_gateway_base_url(&provider.base_url)));
    env.insert("ANTHROPIC_API_KEY".into(), Value::String(secret.into()));
    env.insert("CLAUDE_CODE_ENABLE_GATEWAY_MODEL_DISCOVERY".into(), Value::String("0".into()));
    env.insert("CLAUDE_CODE_SUBAGENT_MODEL".into(), Value::String("inherit".into()));
    env.remove("ANTHROPIC_MODEL");
    for stale_variable in [
        "ANTHROPIC_DEFAULT_FABLE_MODEL",
        "ANTHROPIC_DEFAULT_FABLE_MODEL_NAME",
        "ANTHROPIC_DEFAULT_FABLE_MODEL_DESCRIPTION",
        "ANTHROPIC_DEFAULT_FABLE_MODEL_SUPPORTED_CAPABILITIES",
        "ANTHROPIC_CUSTOM_MODEL_OPTION",
        "ANTHROPIC_CUSTOM_MODEL_OPTION_NAME",
        "ANTHROPIC_CUSTOM_MODEL_OPTION_DESCRIPTION",
        "ANTHROPIC_CUSTOM_MODEL_OPTION_SUPPORTED_CAPABILITIES",
    ] {
        env.remove(stale_variable);
    }
    if let Some(model) = &provider.default_model {
        let profile = provider.claude_model_profile.clone().unwrap_or_else(|| infer_claude_profile(model));
        let mut mappings = provider.claude_model_mappings.clone();
        set_mapping(&mut mappings, &profile, model.clone());
        if mappings.sonnet.is_none() { mappings.sonnet = Some(model.clone()); }
        if mappings.opus.is_none() { mappings.opus = Some(model.clone()); }
        if mappings.haiku.is_none() { mappings.haiku = Some(model.clone()); }
        let selected_alias = match profile {
            ClaudeModelProfile::Sonnet => "sonnet",
            ClaudeModelProfile::Opus => "opus",
            ClaudeModelProfile::Haiku => "haiku",
        };
        let mut model_overrides = Map::new();
        for (slot, variable) in [
            (ClaudeModelProfile::Sonnet, "ANTHROPIC_DEFAULT_SONNET_MODEL"),
            (ClaudeModelProfile::Opus, "ANTHROPIC_DEFAULT_OPUS_MODEL"),
            (ClaudeModelProfile::Haiku, "ANTHROPIC_DEFAULT_HAIKU_MODEL"),
        ] {
            let name_variable = format!("{variable}_NAME");
            if let Some(mapped_model) = mapping_for(&mappings, &slot) {
                let supports_extended = !matches!(slot, ClaudeModelProfile::Haiku);
                let official_model = claude_official_model(&slot);
                let pinned_model = if provider.claude_extended_context && slot == profile && supports_extended { format!("{official_model}[1m]") } else { official_model.to_string() };
                env.insert(variable.into(), Value::String(pinned_model));
                env.insert(name_variable, Value::String(display_model_name(provider, &mapped_model)));
                env.insert(format!("{variable}_DESCRIPTION"), Value::String(format!("{} via {}", display_model_name(provider, &mapped_model), provider.name)));
                env.remove(&format!("{variable}_SUPPORTED_CAPABILITIES"));
                for model_id in claude_official_model_variants(&slot) {
                    model_overrides.insert(model_id.to_string(), Value::String(mapped_model.clone()));
                    model_overrides.insert(format!("{model_id}[1m]"), Value::String(mapped_model.clone()));
                }
            } else {
                env.remove(variable);
                env.remove(&name_variable);
                env.remove(&format!("{variable}_DESCRIPTION"));
                env.remove(&format!("{variable}_SUPPORTED_CAPABILITIES"));
            }
        }
        if let Some(context_window) = provider.models.iter().find(|item| &item.id == model).and_then(|item| item.context_window) {
            env.insert("CLAUDE_CODE_MAX_CONTEXT_TOKENS".into(), Value::String(context_window.to_string()));
        } else {
            env.remove("CLAUDE_CODE_MAX_CONTEXT_TOKENS");
        }
        object.insert("modelOverrides".into(), Value::Object(model_overrides));
        let available_models = vec!["sonnet".to_string(), "opus".to_string(), "haiku".to_string()];
        object.insert("model".into(), Value::String(selected_alias.into()));
        object.insert("availableModels".into(), Value::Array(available_models.into_iter().map(Value::String).collect()));
    }
    serde_json::to_string_pretty(&root).map_err(|e| AppError::Config(e.to_string()))
}

fn anthropic_gateway_base_url(base_url: &str) -> String {
    let Ok(mut url) = Url::parse(base_url) else { return base_url.trim_end_matches('/').to_string(); };
    let path = url.path().trim_end_matches('/').to_string();
    if path.ends_with("/v1") {
        let parent = path[..path.len() - 3].trim_end_matches('/');
        url.set_path(parent);
    } else {
        url.set_path(&path);
    }
    url.to_string().trim_end_matches('/').to_string()
}

fn claude_official_model(profile: &ClaudeModelProfile) -> &'static str {
    match profile {
        ClaudeModelProfile::Sonnet => "claude-sonnet-5",
        ClaudeModelProfile::Opus => "claude-opus-5",
        ClaudeModelProfile::Haiku => "claude-haiku-4-5",
    }
}

fn claude_official_model_variants(profile: &ClaudeModelProfile) -> &'static [&'static str] {
    match profile {
        ClaudeModelProfile::Sonnet => &["claude-sonnet-5", "claude-sonnet-4-6", "claude-sonnet-4-5", "claude-sonnet-4-5-20250929"],
        ClaudeModelProfile::Opus => &["claude-opus-5", "claude-opus-4-8", "claude-opus-4-7", "claude-opus-4-6", "claude-opus-4-5", "claude-opus-4-5-20251101"],
        ClaudeModelProfile::Haiku => &["claude-haiku-4-5", "claude-haiku-4-5-20251001", "claude-haiku-4-5-20251001-v1"],
    }
}

fn mapping_for(mappings: &ClaudeModelMappings, profile: &ClaudeModelProfile) -> Option<String> {
    match profile {
        ClaudeModelProfile::Sonnet => mappings.sonnet.clone(),
        ClaudeModelProfile::Opus => mappings.opus.clone(),
        ClaudeModelProfile::Haiku => mappings.haiku.clone(),
    }
}

fn set_mapping(mappings: &mut ClaudeModelMappings, profile: &ClaudeModelProfile, model: String) {
    match profile {
        ClaudeModelProfile::Sonnet => mappings.sonnet = Some(model),
        ClaudeModelProfile::Opus => mappings.opus = Some(model),
        ClaudeModelProfile::Haiku => mappings.haiku = Some(model),
    }
}

fn display_model_name(provider: &Provider, model: &str) -> String {
    provider.models.iter().find(|item| item.id == model).map(|item| {
        if item.display_name.trim().is_empty() { item.id.clone() } else { item.display_name.clone() }
    }).unwrap_or_else(|| model.into())
}

fn infer_claude_profile(model: &str) -> ClaudeModelProfile {
    let model = model.to_ascii_lowercase();
    if model.contains("opus") { ClaudeModelProfile::Opus }
    else if model.contains("haiku") { ClaudeModelProfile::Haiku }
    else { ClaudeModelProfile::Sonnet }
}

fn merge_opencode(existing: &str, provider: &Provider, secret: &str) -> AppResult<String> {
    let mut root: Value = if existing.trim().is_empty() { json!({ "$schema": "https://opencode.ai/config.json" }) } else { serde_json::from_str(existing).map_err(|e| AppError::Config(format!("OpenCode JSON 解析失败：{e}。若现有文件包含注释，请改用只生成模式。")))? };
    let object = root.as_object_mut().ok_or_else(|| AppError::Config("OpenCode 配置顶层必须是对象".into()))?;
    let providers = object.entry("provider").or_insert_with(|| json!({})).as_object_mut().ok_or_else(|| AppError::Config("OpenCode provider 字段必须是对象".into()))?;
    let id = slug(provider);
    let mut models = Map::new();
    for model in &provider.models { models.insert(model.id.clone(), json!({ "name": model.display_name })); }
    providers.insert(id.clone(), json!({
        "npm": "@ai-sdk/openai-compatible",
        "name": provider.name,
        "options": { "baseURL": provider.base_url, "apiKey": secret },
        "models": models
    }));
    if let Some(model) = &provider.default_model { object.insert("model".into(), Value::String(format!("{id}/{model}"))); }
    serde_json::to_string_pretty(&root).map_err(|e| AppError::Config(e.to_string()))
}

fn generated(client_id: &str, existing: &str, provider: &Provider, secret: &str, settings: &AppSettings) -> AppResult<String> {
    match client_id {
        "codex-cli" => merge_codex(existing, provider, secret, settings),
        "claude-code" => merge_claude(existing, provider, secret),
        "opencode" => merge_opencode(existing, provider, secret),
        _ => Err(AppError::Config("该客户端仅支持手动配置".into())),
    }
}

fn compatible(client_id: &str, protocol: &ProtocolKind) -> bool {
    match client_id {
        "codex-cli" => matches!(protocol, ProtocolKind::Openai | ProtocolKind::AzureOpenai),
        "claude-code" => matches!(protocol, ProtocolKind::Anthropic),
        "opencode" => true,
        _ => false,
    }
}

fn can_configure(client_id: &str, provider: &Provider) -> bool {
    compatible(client_id, &provider.protocol)
        && !(client_id == "codex-cli" && matches!(provider.codex_compatibility, CodexCompatibility::ResponsesUnsupported))
}

pub fn preview(provider: &Provider, client_ids: &[String], settings: &AppSettings) -> AppResult<Vec<ConfigChange>> {
    let catalog = clients::detect_all();
    client_ids.iter().map(|client_id| {
        let client = catalog.iter().find(|item| &item.id == client_id).ok_or_else(|| AppError::InvalidInput(format!("未知客户端：{client_id}")))?;
        let path = clients::config_path(client_id);
        let existing = path.as_deref().map(read_text).transpose()?.unwrap_or_default();
        let can_write = client.auto_config && can_configure(client_id, provider);
        let after = if can_write { generated(client_id, &existing, provider, "<API_KEY：写入时从系统凭据库读取>", settings)? } else { client.guidance.clone() };
        let mut warnings = Vec::new();
        if can_write { warnings.push("该客户端需要在配置文件中保存密钥。仅在关闭“只生成配置”并确认后才会写入明文，文件权限将尽可能收紧。".into()); }
        if client_id == "codex-cli" {
            match provider.codex_compatibility {
                CodexCompatibility::Full => warnings.push("已验证网关支持 Responses API 和 custom 工具，将保留完整的自由格式补丁能力。".into()),
                CodexCompatibility::FunctionToolsOnly => warnings.push("网关不接受 custom 工具，已关闭自由格式补丁声明并使用 Codex 默认兼容工具；复杂补丁可能需要更多轮操作。".into()),
                CodexCompatibility::ChatProxy => warnings.push("已启用本机 Responses 兼容桥。Codex 仍使用 responses 协议，但 namespace、内置搜索、文件输入和服务端会话状态等能力无法由 Chat 后端无损模拟；Provider Deck 必须保持运行。".into()),
                CodexCompatibility::Unknown => warnings.push("兼容性探测因超时、限流或服务异常未完成，已关闭自由格式补丁声明并采用保守模式；首次使用请先在测试目录验证。".into()),
                CodexCompatibility::ResponsesUnsupported => warnings.push("该网关不支持当前 Codex 必需的 Responses 工具协议。官方 Codex 不支持 chat 作为 wire_api，因此不会写入无效配置。".into()),
                CodexCompatibility::NotApplicable => {}
            }
            // 推理档位为什么是这个值、或者为什么被省略，都要让用户在预览里看到，
            // 而不是只留在调试信息里。
            if let Some(model) = &provider.default_model {
                warnings.push(format!("推理档位：{}", codex_reasoning(provider, model, settings).reason));
            }
        }
        if client_id == "opencode" && path.as_ref().is_some_and(|p| p.extension().and_then(|e| e.to_str()) == Some("jsonc")) { warnings.push("JSONC 注释可能无法保留，建议保持只生成模式。".into()); }
        if !can_write { warnings.push(client.guidance.clone()); }
        Ok(ConfigChange {
            client_id: client_id.clone(), client_name: client.name.clone(), target_path: path.as_ref().map(|p| p.to_string_lossy().into_owned()),
            support: client.support.clone(), can_write, format: match client_id.as_str() { "codex-cli" => "toml", "claude-code" | "opencode" => "json", _ => "manual" }.into(),
            before_preview: if existing.is_empty() { "（新建配置文件）".into() } else { redact_preview(&existing) },
            after_preview: if can_write { redact_preview(&after) } else { after }, warnings,
            expected_hash: path.as_deref().map(file_hash).transpose()?,
        })
    }).collect()
}

fn redact_preview(content: &str) -> String {
    content.lines().take(80).map(|line| {
        let lower = line.to_ascii_lowercase();
        if lower.contains("api_key") || lower.contains("apikey") || lower.contains("bearer_token") { line.split_once('=').map(|(left, _)| format!("{left}= \"[REDACTED]\"")).unwrap_or_else(|| "[REDACTED SECRET FIELD]".into()) } else { line.into() }
    }).collect::<Vec<String>>().join("\n")
}

pub fn apply(provider: &Provider, secret: &str, changes: &[ConfigChange], settings: &AppSettings) -> AppResult<(Vec<crate::model::ApplyResult>, Vec<BackupRecord>)> {
    let mut results = Vec::new();
    let mut backups = Vec::new();
    for change in changes {
        if !change.can_write || !can_configure(&change.client_id, provider) {
            results.push(crate::model::ApplyResult { client_id: change.client_id.clone(), success: false, backup_id: None, message: "该客户端仅提供手动配置指引".into(), restart_required: true });
            continue;
        }
        if settings.generate_only {
            results.push(crate::model::ApplyResult { client_id: change.client_id.clone(), success: true, backup_id: None, message: "配置已生成并预览；只生成模式未写入文件".into(), restart_required: false });
            continue;
        }
        let path = clients::config_path(&change.client_id).ok_or_else(|| AppError::Config("缺少目标配置路径".into()))?;
        let current_hash = file_hash(&path)?;
        if change.expected_hash.as_deref() != Some(current_hash.as_str()) { return Err(AppError::ExternalModification); }
        let existing = read_text(&path)?;
        let output = generated(&change.client_id, &existing, provider, secret, settings)?;
        let backup = create_backup(&change.client_id, &path)?;
        let catalog_artifact = if change.client_id == "codex-cli" { Some((codex_catalog_path()?, codex_catalog(provider, settings)?)) } else { None };
        let catalog_backup = catalog_artifact.as_ref().map(|(catalog_path, _)| create_backup("codex-cli-model-catalog", catalog_path)).transpose()?;
        if let Some((catalog_path, catalog)) = &catalog_artifact {
            if let Err(error) = atomic_replace(catalog_path, catalog.as_bytes()).and_then(|_| restrict_permissions(catalog_path)) {
                let _ = restore_record(&backup);
                if let Some(record) = &catalog_backup { let _ = restore_record(record); }
                return Err(error);
            }
        }
        if let Err(error) = atomic_replace(&path, output.as_bytes()).and_then(|_| restrict_permissions(&path)) {
            let _ = restore_record(&backup);
            if let Some(record) = &catalog_backup { let _ = restore_record(record); }
            return Err(error);
        }
        results.push(crate::model::ApplyResult { client_id: change.client_id.clone(), success: true, backup_id: Some(backup.id.clone()), message: "备份和原子写入完成".into(), restart_required: true });
        backups.push(backup);
        if let Some(record) = catalog_backup { backups.push(record); }
    }
    Ok((results, backups))
}

fn create_backup(client_id: &str, target: &Path) -> AppResult<BackupRecord> {
    let dirs = ProjectDirs::from("cn", "ProviderDeck", "Provider Deck").ok_or_else(|| AppError::Config("无法确定备份目录".into()))?;
    let backup_dir = dirs.data_dir().join("backups").join(client_id);
    fs::create_dir_all(&backup_dir)?;
    let id = Uuid::new_v4().to_string();
    let existed = target.exists();
    let backup_path = backup_dir.join(format!("{}-{}.bak", Utc::now().format("%Y%m%dT%H%M%S%.3fZ"), id));
    let bytes = if existed { fs::read(target)? } else { Vec::new() };
    fs::write(&backup_path, &bytes)?;
    Ok(BackupRecord { id, client_id: client_id.into(), target_path: target.to_string_lossy().into_owned(), backup_path: backup_path.to_string_lossy().into_owned(), created_at: Utc::now().to_rfc3339(), size: bytes.len() as u64, original_exists: existed })
}

pub fn restore_record(record: &BackupRecord) -> AppResult<()> {
    let target = PathBuf::from(&record.target_path);
    if record.original_exists { atomic_replace(&target, &fs::read(&record.backup_path)?)?; restrict_permissions(&target)?; }
    else if target.exists() { fs::remove_file(target)?; }
    Ok(())
}

#[cfg(unix)]
fn restrict_permissions(path: &Path) -> AppResult<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    Ok(())
}

#[cfg(not(unix))]
fn restrict_permissions(_path: &Path) -> AppResult<()> { Ok(()) }

#[cfg(test)]
mod tests {
    use super::*;
    fn provider() -> Provider { Provider { id: "12345678-test".into(), name: "Example".into(), base_url: "https://api.example.com/v1".into(), protocol: ProtocolKind::Openai, enabled: true, is_current: true, default_model: Some("coder".into()), claude_model_profile: None, claude_extended_context: false, claude_model_mappings: ClaudeModelMappings::default(), codex_compatibility: CodexCompatibility::Full, codex_probe_model: Some("coder".into()), codex_probe_detail: None, reasoning_selections: vec![], models: vec![], connection_state: "connected".into(), confidence: Some(0.9), last_checked_at: None, applied_clients: vec![], error_summary: None } }
    #[test]
    fn codex_merge_preserves_unknown_fields() {
        let output = merge_codex("custom_flag = true\n", &provider(), "secret", &AppSettings::default()).unwrap();
        assert!(output.contains("custom_flag = true"));
        let document = DocumentMut::from_str(&output).unwrap();
        assert_eq!(document["model_providers"]["example"]["name"].as_str(), Some("Example"));
        assert_eq!(document["model_context_window"].as_integer(), Some(200_000));
        assert_eq!(document["model_reasoning_effort"].as_str(), Some("high"));
        assert_eq!(document["model_supports_reasoning_summaries"].as_bool(), Some(false));
        assert!(document["model_catalog_json"].as_str().is_some_and(|path| path.ends_with("provider-deck-model-catalog.json")));
        let catalog: Value = serde_json::from_str(&codex_catalog(&provider(), &AppSettings::default()).unwrap()).unwrap();
        assert_eq!(catalog["models"][0]["slug"], "coder");
        assert_eq!(catalog["models"][0]["default_reasoning_level"], "high");
        // 硬编码的 minimal/low/medium/high 四项清单已删除。这个 Provider 没有任何已发现的
        // 能力，就没有"服务端声明过的档位"可写——不编造清单，但上面的全局档位仍然保留，
        // 旧配置不失效。
        assert!(catalog["models"][0].get("supported_reasoning_levels").is_none());
        assert_eq!(catalog["models"][0]["apply_patch_tool_type"], "freeform");
    }

    /// 便于测试构造能力：档位成员原样来自入参，模拟服务端声明。
    fn effort_capability(model_id: &str, values: &[&str]) -> ReasoningCapability {
        ReasoningCapability::from_effort_enum(
            crate::reasoning_capability::ReasoningKey::new("https://api.example.com/v1", model_id),
            &values.iter().map(|value| (*value).to_string()).collect::<Vec<_>>(),
            crate::reasoning_capability::ReasoningConfidence::Declared,
        )
    }

    fn model_with(model_id: &str, reasoning: Option<ReasoningCapability>) -> crate::model::ModelInfo {
        crate::model::ModelInfo {
            id: model_id.into(), display_name: model_id.into(), provider: None,
            protocol: ProtocolKind::Openai, source: "test".into(), capabilities: Vec::new(),
            context_window: None, parameter_count_billions: None, reasoning,
        }
    }

    /// 用户选中的档位必须出现在 config.toml 与 catalog 里。
    #[test]
    fn effort_selection_updates_codex_reasoning_effort() {
        use crate::reasoning_capability::ReasoningTier;
        use crate::reasoning_selection::{ReasoningSelection, SelectionSource};

        let mut provider = provider();
        provider.models = vec![model_with("coder", Some(effort_capability("coder", &["low", "medium", "high"])))];
        // medium 在这张能力表里是 Standard 档。
        provider.reasoning_selections = vec![ReasoningSelection::new("coder", ReasoningTier::Standard, SelectionSource::User)];

        let settings = AppSettings::default();
        let document = DocumentMut::from_str(&merge_codex("", &provider, "secret", &settings).unwrap()).unwrap();
        assert_eq!(document["model_reasoning_effort"].as_str(), Some("medium"), "用户选择没有进入 config.toml");

        let catalog: Value = serde_json::from_str(&codex_catalog(&provider, &settings).unwrap()).unwrap();
        assert_eq!(catalog["models"][0]["default_reasoning_level"], "medium");
    }

    /// 旧 state.json 没有 reasoningSelections，此时全局 manual/effective 档位仍然说话。
    #[test]
    fn legacy_global_reasoning_level_still_works() {
        let mut provider = provider();
        provider.models = vec![model_with("coder", Some(effort_capability("coder", &["low", "medium", "high"])))];
        assert!(provider.reasoning_selections.is_empty());

        let settings = AppSettings {
            manual_reasoning_level: crate::model::ReasoningLevel::High,
            effective_reasoning_level: crate::model::ReasoningLevel::High,
            ..AppSettings::default()
        };

        let document = DocumentMut::from_str(&merge_codex("", &provider, "secret", &settings).unwrap()).unwrap();
        assert_eq!(document["model_reasoning_effort"].as_str(), Some("high"), "升级后旧用户的档位失效了");
    }

    /// 预算型绑定不得被硬塞进 model_reasoning_effort。
    #[test]
    fn budget_binding_is_not_written_as_numeric_effort() {
        let mut provider = provider();
        provider.protocol = ProtocolKind::Anthropic;
        let capability = ReasoningCapability::from_token_budget(
            crate::reasoning_capability::ReasoningKey::new("https://api.example.com/v1", "coder"),
            1024, 8192, false, None,
            crate::reasoning_capability::ReasoningConfidence::Validated,
        );
        // 前提校验：这张能力表确实产出了 Budget 绑定，否则本测试什么都没测到。
        assert!(capability.tiers.iter().any(|tier| matches!(tier.binding, ReasoningBinding::Budget { .. })));
        provider.models = vec![model_with("coder", Some(capability))];

        let settings = AppSettings::default();
        let resolved = codex_reasoning(&provider, "coder", &settings);
        assert_eq!(resolved.effort, None, "预算型绑定被写成了 effort：{:?}", resolved.effort);
        assert!(
            resolved.reason.contains("budget binding cannot be represented by Codex model_reasoning_effort"),
            "缺少省略原因说明：{}", resolved.reason
        );

        let output = merge_codex("", &provider, "secret", &settings).unwrap();
        assert!(!output.contains("8192"), "预算数字泄漏进了 Codex 配置：{output}");
        let document = DocumentMut::from_str(&output).unwrap();
        assert!(document.as_table().get("model_reasoning_effort").is_none());
    }

    /// 探到"不支持推理"时不写任何推理字段。
    #[test]
    fn unsupported_capability_omits_reasoning_field() {
        let mut provider = provider();
        provider.models = vec![model_with("coder", Some(ReasoningCapability::unsupported(
            crate::reasoning_capability::ReasoningKey::new("https://api.example.com/v1", "coder"),
            crate::reasoning_capability::ReasoningConfidence::Validated,
        )))];

        let settings = AppSettings::default();
        let document = DocumentMut::from_str(&merge_codex("", &provider, "secret", &settings).unwrap()).unwrap();
        assert!(document.as_table().get("model_reasoning_effort").is_none(), "不支持推理的模型仍被写入了档位");

        let catalog: Value = serde_json::from_str(&codex_catalog(&provider, &settings).unwrap()).unwrap();
        assert!(catalog["models"][0].get("default_reasoning_level").is_none());
        assert!(catalog["models"][0].get("supported_reasoning_levels").is_none());
    }

    /// 已存的旧档位必须被删掉，不能以一个此模型并不支持的取值继续生效。
    #[test]
    fn stale_effort_is_removed_when_model_cannot_express_it() {
        let mut provider = provider();
        provider.models = vec![model_with("coder", Some(ReasoningCapability::unsupported(
            crate::reasoning_capability::ReasoningKey::new("https://api.example.com/v1", "coder"),
            crate::reasoning_capability::ReasoningConfidence::Validated,
        )))];

        let output = merge_codex("model_reasoning_effort = \"high\"\n", &provider, "secret", &AppSettings::default()).unwrap();
        assert!(!output.contains("model_reasoning_effort"), "旧档位残留：{output}");
    }

    /// 档位清单完全由服务端声明的成员派生，未来新增成员自动出现。
    #[test]
    fn dynamic_reasoning_levels_have_no_hardcode() {
        let mut provider = provider();
        // ultra 不在任何内置等级表里，minimal/medium 在，xhigh 介于两者之间。
        provider.models = vec![model_with("coder", Some(effort_capability("coder", &["minimal", "medium", "xhigh", "ultra"])))];

        let catalog: Value = serde_json::from_str(&codex_catalog(&provider, &AppSettings::default()).unwrap()).unwrap();
        let levels = catalog["models"][0]["supported_reasoning_levels"].as_array().cloned().unwrap_or_default();
        let efforts = levels.iter().filter_map(|item| item["effort"].as_str()).collect::<Vec<_>>();

        assert_eq!(efforts, vec!["minimal", "medium", "xhigh", "ultra"], "档位清单没有按服务端声明原样输出");
        assert!(levels.iter().all(|item| item["description"].as_str().is_some_and(|text| !text.is_empty())));
    }
    #[test]
    fn codex_catalog_omits_patch_type_when_custom_probe_is_not_full() {
        let mut provider = provider();
        provider.codex_compatibility = CodexCompatibility::FunctionToolsOnly;
        let catalog: Value = serde_json::from_str(&codex_catalog(&provider, &AppSettings::default()).unwrap()).unwrap();
        assert!(catalog["models"][0].get("apply_patch_tool_type").is_none());
    }
    #[test]
    fn repairs_only_the_legacy_invalid_function_patch_type() {
        let mut catalog = json!({
            "models": [
                { "slug": "broken", "apply_patch_tool_type": "function", "custom": true },
                { "slug": "full", "apply_patch_tool_type": "freeform" }
            ],
            "custom_root": true
        });
        assert!(remove_legacy_function_patch_type(&mut catalog));
        assert!(catalog["models"][0].get("apply_patch_tool_type").is_none());
        assert_eq!(catalog["models"][0]["custom"], true);
        assert_eq!(catalog["models"][1]["apply_patch_tool_type"], "freeform");
        assert_eq!(catalog["custom_root"], true);
        assert!(!remove_legacy_function_patch_type(&mut catalog));
    }
    #[test]
    fn claude_merge_preserves_unknown_fields() {
        let output = merge_claude(r#"{"theme":"dark","env":{"OTHER":"x"}}"#, &provider(), "secret").unwrap();
        let value: Value = serde_json::from_str(&output).unwrap();
        assert_eq!(value["theme"], "dark");
        assert_eq!(value["env"]["OTHER"], "x");
        assert_eq!(value["model"], "sonnet");
        assert_eq!(value["env"]["ANTHROPIC_BASE_URL"], "https://api.example.com");
        assert_eq!(value["env"]["ANTHROPIC_DEFAULT_SONNET_MODEL"], "claude-sonnet-5");
        assert_eq!(value["env"]["ANTHROPIC_DEFAULT_OPUS_MODEL"], "claude-opus-5");
        assert_eq!(value["env"]["ANTHROPIC_DEFAULT_HAIKU_MODEL"], "claude-haiku-4-5");
        assert_eq!(value["env"]["CLAUDE_CODE_ENABLE_GATEWAY_MODEL_DISCOVERY"], "0");
        assert_eq!(value["env"]["CLAUDE_CODE_SUBAGENT_MODEL"], "inherit");
        assert_eq!(value["modelOverrides"]["claude-sonnet-5"], "coder");
        assert_eq!(value["modelOverrides"]["claude-sonnet-5[1m]"], "coder");
        assert_eq!(value["modelOverrides"]["claude-opus-5"], "coder");
        assert_eq!(value["modelOverrides"]["claude-opus-5[1m]"], "coder");
        assert_eq!(value["modelOverrides"]["claude-haiku-4-5"], "coder");
        assert!(value["env"].get("ANTHROPIC_MODEL").is_none());
        assert!(!value["availableModels"].as_array().unwrap().contains(&json!("coder")));
    }
    #[test]
    fn claude_merge_maps_custom_opus_with_extended_context() {
        let mut custom = provider();
        custom.default_model = Some("third-party-coding-model".into());
        custom.claude_model_profile = Some(ClaudeModelProfile::Opus);
        custom.claude_extended_context = true;
        custom.models = vec![crate::model::ModelInfo { id: "third-party-coding-model".into(), display_name: "Third Party Coding Model".into(), provider: None, protocol: ProtocolKind::Anthropic, source: "server".into(), capabilities: vec![], context_window: Some(1_000_000), parameter_count_billions: None, reasoning: None }];
        let output = merge_claude("{}", &custom, "secret").unwrap();
        let value: Value = serde_json::from_str(&output).unwrap();
        assert_eq!(value["model"], "opus");
        assert!(value["env"].get("ANTHROPIC_MODEL").is_none());
        assert_eq!(value["env"]["ANTHROPIC_DEFAULT_OPUS_MODEL"], "claude-opus-5[1m]");
        assert_eq!(value["env"]["ANTHROPIC_DEFAULT_OPUS_MODEL_NAME"], "Third Party Coding Model");
        assert_eq!(value["env"]["CLAUDE_CODE_MAX_CONTEXT_TOKENS"], "1000000");
        assert_eq!(value["modelOverrides"]["claude-opus-5"], "third-party-coding-model");
    }

    #[test]
    fn claude_merge_maps_all_custom_slots_and_replaces_stale_values() {
        let mut custom = provider();
        custom.default_model = Some("agnes-2.0-flash".into());
        custom.claude_model_profile = Some(ClaudeModelProfile::Sonnet);
        custom.claude_model_mappings = ClaudeModelMappings { sonnet: Some("agnes-2.0-flash".into()), opus: Some("deep-coder".into()), haiku: None };
        custom.models = vec![
            crate::model::ModelInfo { id: "agnes-2.0-flash".into(), display_name: "Agnes Flash".into(), provider: None, protocol: ProtocolKind::Anthropic, source: "server".into(), capabilities: vec![], context_window: None, parameter_count_billions: None, reasoning: None },
            crate::model::ModelInfo { id: "deep-coder".into(), display_name: "Deep Coder".into(), provider: None, protocol: ProtocolKind::Anthropic, source: "server".into(), capabilities: vec![], context_window: None, parameter_count_billions: None, reasoning: None },
        ];
        let output = merge_claude(r#"{"env":{"ANTHROPIC_MODEL":"agnes-2.0-flash","ANTHROPIC_DEFAULT_HAIKU_MODEL":"old-model","ANTHROPIC_DEFAULT_HAIKU_MODEL_NAME":"Old","ANTHROPIC_DEFAULT_FABLE_MODEL":"claude-fable-5","ANTHROPIC_CUSTOM_MODEL_OPTION":"old-custom","CLAUDE_CODE_SUBAGENT_MODEL":"old-subagent"}}"#, &custom, "secret").unwrap();
        let value: Value = serde_json::from_str(&output).unwrap();
        assert_eq!(value["model"], "sonnet");
        assert_eq!(value["env"]["ANTHROPIC_DEFAULT_SONNET_MODEL"], "claude-sonnet-5");
        assert_eq!(value["env"]["ANTHROPIC_DEFAULT_SONNET_MODEL_NAME"], "Agnes Flash");
        assert_eq!(value["env"]["ANTHROPIC_DEFAULT_OPUS_MODEL"], "claude-opus-5");
        assert_eq!(value["env"]["ANTHROPIC_DEFAULT_HAIKU_MODEL"], "claude-haiku-4-5");
        assert_eq!(value["modelOverrides"]["claude-sonnet-5"], "agnes-2.0-flash");
        assert_eq!(value["modelOverrides"]["claude-opus-5"], "deep-coder");
        assert_eq!(value["modelOverrides"]["claude-haiku-4-5"], "agnes-2.0-flash");
        assert_eq!(value["env"]["CLAUDE_CODE_SUBAGENT_MODEL"], "inherit");
        assert!(value["env"].get("ANTHROPIC_DEFAULT_FABLE_MODEL").is_none());
        assert!(value["env"].get("ANTHROPIC_CUSTOM_MODEL_OPTION").is_none());
    }

    #[test]
    fn claude_gateway_base_url_removes_only_the_messages_v1_suffix() {
        assert_eq!(anthropic_gateway_base_url("https://gateway.example.com/v1"), "https://gateway.example.com");
        assert_eq!(anthropic_gateway_base_url("https://gateway.example.com/anthropic/v1/"), "https://gateway.example.com/anthropic");
        assert_eq!(anthropic_gateway_base_url("https://gateway.example.com/anthropic"), "https://gateway.example.com/anthropic");
    }
}
