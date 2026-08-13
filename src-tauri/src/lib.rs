mod activity;
mod chat_store;
mod clients;
mod config;
mod credentials;
mod error;
mod launcher;
mod model;
mod local_proxy;
mod responses_chat;
mod protocol;
mod redaction;
mod reasoning;
mod reasoning_capability;
mod reasoning_adapters;
mod reasoning_discovery;
mod reasoning_selection;
mod reasoning_verification;
mod storage;

use std::collections::HashMap;
use chrono::Utc;
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Manager, State, WindowEvent,
};
use uuid::Uuid;
use error::{AppError, AppResult};
use chat_store::{ChatBackupRecord, ChatCacheSummary, ChatRestoreMode, ChatRestoreResult, ChatStore};
use model::{
    AppSettings, ApplyResult, BackupRecord, ClientDescriptor, ConfigChange, MatchedCustomTier, ModelReasoningMeta,
    ProbeResult, Provider, ProviderDraft, ProviderTestReport, ReasoningDetectionCacheEntry,
};
use local_proxy::LocalProxy;
use storage::StateStore;

fn refresh_current_reasoning(state: &mut model::PersistedState, log_action: bool) {
    reasoning::refresh_settings(&mut state.settings, log_action);
}

#[tauri::command]
fn list_providers(store: State<'_, StateStore>) -> Vec<Provider> { store.read().providers }

#[tauri::command]
fn get_provider_api_key(store: State<'_, StateStore>, provider_id: String) -> AppResult<String> {
    if !store.read().providers.iter().any(|provider| provider.id == provider_id) {
        return Err(AppError::ProviderNotFound(provider_id));
    }
    credentials::get(&provider_id)
}

#[tauri::command]
async fn save_provider(store: State<'_, StateStore>, proxy: State<'_, LocalProxy>, draft: ProviderDraft, mut probe: ProbeResult) -> AppResult<Provider> {
    let mut resolved_draft = draft;
    let id = resolved_draft.id.clone().unwrap_or_else(|| Uuid::new_v4().to_string());
    if resolved_draft.api_key.trim().is_empty() {
        if resolved_draft.id.is_none() {
            return Err(AppError::InvalidInput("API Key 不能为空".into()));
        }
        resolved_draft.api_key = credentials::get(&id)?;
    }
    credentials::set(&id, &resolved_draft.api_key)?;
    let settings = store.read().settings;
    // 回灌已存的推理能力，作为发现流程的缓存输入：未过期就不会再发请求。
    // 用探测归一化后的 base_url 判定归属——拿用户原始输入去比会让每次保存都失配，
    // 把一个正确性问题换成"每次保存重发一轮 Tier 2 探测"的成本问题。
    if let Some(stored) = store.read().providers.iter().find(|provider| provider.id == id) {
        let target_base_url = probe.normalized_base_url.clone();
        carry_reasoning_forward(&mut probe.models, &stored.models, &target_base_url);
    }
    protocol::refresh_selected_capabilities(&resolved_draft, &settings, &mut probe).await;
    let provider = store.update(|state| {
        let existing = state.providers.iter().find(|provider| provider.id == id).cloned();
        let provider = Provider {
            id: id.clone(), name: resolved_draft.name.trim().into(), base_url: probe.normalized_base_url.clone(), protocol: probe.protocol.clone(),
            enabled: existing.as_ref().map(|p| p.enabled).unwrap_or(true),
            is_current: existing.as_ref().map(|p| p.is_current).unwrap_or(state.providers.is_empty()),
            default_model: resolved_draft.default_model.clone().filter(|model| probe.models.is_empty() || probe.models.iter().any(|item| &item.id == model))
                .or_else(|| existing.as_ref().and_then(|p| p.default_model.clone()).filter(|model| probe.models.is_empty() || probe.models.iter().any(|item| &item.id == model)))
                .or_else(|| probe.models.first().map(|m| m.id.clone())),
            claude_model_profile: resolved_draft.claude_model_profile.clone().or_else(|| existing.as_ref().and_then(|p| p.claude_model_profile.clone())),
            claude_extended_context: resolved_draft.claude_extended_context,
            claude_model_mappings: resolved_draft.claude_model_mappings.clone(),
            codex_compatibility: probe.codex_compatibility.clone(),
            codex_probe_model: probe.codex_probe_model.clone(),
            codex_probe_detail: probe.codex_probe_detail.clone(),
            // 草稿优先、草稿未提到的模型保留原值，最后剪掉已消失的模型。
            // 前端只提交正在编辑的那一个模型时，其他模型的选择不能被清掉。
            reasoning_selections: {
                let existing = existing.as_ref().map(|p| p.reasoning_selections.as_slice()).unwrap_or(&[]);
                let mut merged = reasoning_selection::merge_drafted(existing, &resolved_draft.reasoning_selections);
                reasoning_selection::prune_missing(&mut merged, &probe.models);
                merged
            },
            // 验证记录绑定 (base_url, model_id)：端点变了旧记录一律作废，
            // 端点没变则只剪掉已消失的模型。与 reasoning_selections 的"只按 model_id 剪枝"
            // 是刻意的不对称——选择是用户意图，验证是对某个端点的事实断言。
            reasoning_verifications: {
                let mut carried = existing.as_ref().map(|p| p.reasoning_verifications.clone()).unwrap_or_default();
                reasoning_verification::retain_for_endpoint(&mut carried, &probe.normalized_base_url, &probe.models);
                carried
            },
            models: probe.models.clone(), connection_state: "connected".into(), confidence: Some(probe.confidence),
            last_checked_at: Some(Utc::now().to_rfc3339()), applied_clients: existing.map(|p| p.applied_clients).unwrap_or_default(), error_summary: None,
        };
        state.providers.retain(|item| item.id != id);
        state.providers.push(provider.clone());
        refresh_current_reasoning(state, true);
        Ok(provider)
    })?;
    let effective_settings = store.read().settings;
    if matches!(provider.codex_compatibility, model::CodexCompatibility::ChatProxy) {
        let token = credentials::proxy_token(&provider.id)?;
        proxy.register(&provider, &resolved_draft.api_key, &token, &effective_settings)?;
    } else {
        proxy.unregister(&provider.id);
    }
    Ok(provider)
}

#[tauri::command]
fn delete_provider(store: State<'_, StateStore>, proxy: State<'_, LocalProxy>, id: String) -> AppResult<()> {
    store.update(|state| { state.providers.retain(|provider| provider.id != id); Ok(()) })?;
    proxy.unregister(&id);
    credentials::delete(&id)
}

#[tauri::command]
fn set_current_provider(store: State<'_, StateStore>, proxy: State<'_, LocalProxy>, id: String) -> AppResult<Vec<Provider>> {
    let providers = store.update(|state| {
        if !state.providers.iter().any(|provider| provider.id == id) { return Err(AppError::ProviderNotFound(id.clone())); }
        for provider in &mut state.providers { provider.is_current = provider.id == id; }
        refresh_current_reasoning(state, true);
        Ok(state.providers.clone())
    })?;
    let state = store.read();
    if let Some(provider) = state.providers.iter().find(|provider| provider.is_current)
        .filter(|provider| matches!(provider.codex_compatibility, model::CodexCompatibility::ChatProxy)) {
        proxy.register(provider, &credentials::get(&provider.id)?, &credentials::proxy_token(&provider.id)?, &state.settings)?;
    }
    activity::record("provider_switch", &format!("切换当前 Provider：{id}"), true);
    Ok(providers)
}

#[tauri::command]
async fn probe_provider(store: State<'_, StateStore>, draft: ProviderDraft) -> AppResult<ProbeResult> {
    let mut resolved_draft = draft;
    if resolved_draft.api_key.trim().is_empty() {
        let provider_id = resolved_draft.id.as_deref().ok_or_else(|| AppError::InvalidInput("API Key 不能为空".into()))?;
        resolved_draft.api_key = credentials::get(provider_id)?;
    }
    protocol::probe(&resolved_draft, &store.read().settings).await
}

#[tauri::command]
async fn reprobe_provider(store: State<'_, StateStore>, proxy: State<'_, LocalProxy>, id: String) -> AppResult<Provider> {
    let state = store.read();
    let provider = state.providers.iter().find(|provider| provider.id == id).cloned()
        .ok_or_else(|| AppError::ProviderNotFound(id.clone()))?;
    let settings = state.settings.clone();
    let draft = ProviderDraft {
        id: Some(provider.id.clone()),
        name: provider.name.clone(),
        base_url: provider.base_url.clone(),
        api_key: credentials::get(&provider.id)?,
        protocol_hint: Some(provider.protocol.clone()),
        timeout_seconds: settings.timeout_seconds,
        azure_api_version: None,
        default_model: provider.default_model.clone(),
        claude_model_profile: provider.claude_model_profile.clone(),
        claude_extended_context: provider.claude_extended_context,
        claude_model_mappings: provider.claude_model_mappings.clone(),
        // 重探测不改用户意图：选择由 store 里的那份权威，这里不回灌也不覆盖。
        reasoning_selections: Vec::new(),
    };

    match protocol::probe(&draft, &settings).await {
        Ok(probe) => {
            let refreshed = store.update(|state| {
                let refreshed = {
                    let saved = state.providers.iter_mut().find(|item| item.id == id)
                        .ok_or_else(|| AppError::ProviderNotFound(id.clone()))?;
                    // 先接住归一化后的 base_url：能力迁移必须按**新**端点判定，
                    // 单独存一份避免依赖赋值语句的先后顺序。
                    let normalized_base_url = probe.normalized_base_url;
                    saved.base_url = normalized_base_url.clone();
                    saved.protocol = probe.protocol;
                    let mut models = probe.models;
                    carry_reasoning_forward(&mut models, &saved.models, &normalized_base_url);
                    saved.models = models;
                    saved.codex_compatibility = probe.codex_compatibility;
                    saved.codex_probe_model = probe.codex_probe_model;
                    saved.codex_probe_detail = probe.codex_probe_detail;
                    saved.default_model = saved.default_model.clone().filter(|model| saved.models.iter().any(|item| &item.id == model)).or_else(|| saved.models.first().map(|model| model.id.clone()));
                    saved.claude_model_mappings.sonnet = saved.claude_model_mappings.sonnet.clone().filter(|model| saved.models.iter().any(|item| &item.id == model));
                    saved.claude_model_mappings.opus = saved.claude_model_mappings.opus.clone().filter(|model| saved.models.iter().any(|item| &item.id == model));
                    saved.claude_model_mappings.haiku = saved.claude_model_mappings.haiku.clone().filter(|model| saved.models.iter().any(|item| &item.id == model));
                    // 只剪掉真的消失了的模型：换端点不影响"用户想要什么档位"。
                    let models_snapshot = saved.models.clone();
                    reasoning_selection::prune_missing(&mut saved.reasoning_selections, &models_snapshot);
                    // 验证记录相反：端点变了就全作废，因为它断言的是某个端点的运行时行为。
                    let verification_base_url = saved.base_url.clone();
                    reasoning_verification::retain_for_endpoint(&mut saved.reasoning_verifications, &verification_base_url, &models_snapshot);
                    saved.connection_state = "connected".into();
                    saved.confidence = Some(probe.confidence);
                    saved.last_checked_at = Some(Utc::now().to_rfc3339());
                    saved.error_summary = None;
                    saved.clone()
                };
                refresh_current_reasoning(state, true);
                Ok(refreshed)
            })?;
            if matches!(refreshed.codex_compatibility, model::CodexCompatibility::ChatProxy) {
                let token = credentials::proxy_token(&refreshed.id)?;
                proxy.register(&refreshed, &draft.api_key, &token, &settings)?;
            } else {
                proxy.unregister(&refreshed.id);
            }
            Ok(refreshed)
        },
        Err(error) => {
            let summary = error.to_string();
            store.update(|state| {
                if let Some(saved) = state.providers.iter_mut().find(|item| item.id == id) {
                    saved.connection_state = "failed".into();
                    saved.error_summary = Some(summary);
                }
                Ok(())
            })?;
            Err(error)
        }
    }
}

/// 把已发现的推理能力从旧模型列表迁移到新模型列表。
///
/// 刷新模型（reprobe / refresh_provider_models / save_provider）都是整体替换 models，
/// 不迁移的话每次刷新都会清空能力缓存：Unknown 的 6 小时退避窗口归零，用户点一次
/// "获取模型"就会在下次保存时重发一轮 Tier 2 探测。
///
/// 能力归属 `(base_url, model_id)`，所以按 model_id 对齐**不够**：同一个 model_id 在
/// 不同端点上是不同的部署，档位、预算上限、甚至支持与否都可能不一样。这里必须拿
/// `target_base_url`（探测归一化后的值，不是用户原始输入）过一遍
/// [`ReasoningKey::matches`]，失配就整条丢弃——capability / confidence / evidence
/// 同生同死，不做部分迁移：留下证据却换掉端点，等于用旧证据为新端点背书。
///
/// 丢弃是安全的：`reasoning: None` 进入发现流程就是 `cached: None`，TTL 短路不成立，
/// 会走一次完整发现，代价是一轮探测而不是一个错误的结论。
///
/// 与之对应，`Provider.reasoning_selections` 跨端点保留，见
/// [`reasoning_selection::prune_missing`]：能力是事实，选择是意图。
fn carry_reasoning_forward(fresh: &mut [model::ModelInfo], previous: &[model::ModelInfo], target_base_url: &str) {
    for model in fresh.iter_mut() {
        if model.reasoning.is_some() { continue; }
        let Some(existing) = previous.iter().find(|item| item.id == model.id) else { continue };
        let Some(capability) = existing.reasoning.as_ref() else { continue };
        if capability.key.matches(target_base_url, &model.id) {
            model.reasoning = Some(capability.clone());
        }
    }
}

fn provider_draft(provider: &Provider, api_key: String, settings: &AppSettings) -> ProviderDraft {
    ProviderDraft {
        id: Some(provider.id.clone()),
        name: provider.name.clone(),
        base_url: provider.base_url.clone(),
        api_key,
        protocol_hint: Some(provider.protocol.clone()),
        timeout_seconds: settings.timeout_seconds,
        azure_api_version: None,
        default_model: provider.default_model.clone(),
        claude_model_profile: provider.claude_model_profile.clone(),
        claude_extended_context: provider.claude_extended_context,
        claude_model_mappings: provider.claude_model_mappings.clone(),
        reasoning_selections: provider.reasoning_selections.clone(),
    }
}

/// 对**单个**模型重新发现推理能力。
///
/// 保存时的自动 discovery 只覆盖默认模型（一次探测的代价换一个结论），其余模型停留在
/// "未探明"。这个命令是用户主动为某个模型付这份代价的入口，不做批量：20 个模型的批量
/// 发现是 20~60 个请求，还会把限流失败当成结论写进 TTL 窗口。
///
/// 实现上不新增发现逻辑，而是喂给现有编排器一个合成的 `ProbeResult`：
/// - `models` 只放目标模型，且 `reasoning` 清空 —— `cached: None`，TTL 短路不成立，强制重新发现
/// - `default_model` 指向目标模型 —— 编排器选中的就是它
/// - `codex_probe_model` 预置为目标模型 —— 命中 `run_codex_probe` 的早退，不会重跑 Codex 探测
///   也不会覆盖已存的兼容性结论
#[tauri::command]
async fn reprobe_model_reasoning(store: State<'_, StateStore>, provider_id: String, model_id: String) -> AppResult<Provider> {
    let state = store.read();
    let provider = state.providers.iter().find(|provider| provider.id == provider_id).cloned()
        .ok_or_else(|| AppError::ProviderNotFound(provider_id.clone()))?;
    let settings = state.settings.clone();
    let mut target = provider.models.iter().find(|model| model.id == model_id).cloned()
        .ok_or_else(|| AppError::InvalidInput(format!("Provider 下没有模型 {model_id}")))?;
    target.reasoning = None;

    let api_key = credentials::get(&provider.id)?;
    let mut draft = provider_draft(&provider, api_key, &settings);
    draft.default_model = Some(model_id.clone());

    let mut probe = ProbeResult {
        normalized_base_url: provider.base_url.clone(),
        protocol: provider.protocol,
        confidence: provider.confidence.unwrap_or(0.0),
        models: vec![target],
        codex_compatibility: provider.codex_compatibility.clone(),
        codex_probe_model: Some(model_id.clone()),
        codex_probe_detail: provider.codex_probe_detail.clone(),
        checked_endpoints: Vec::new(),
        user_message: String::new(),
        technical_detail: None,
        reasoning_note: None,
    };
    protocol::refresh_selected_capabilities(&draft, &settings, &mut probe).await;

    let discovered = probe.models.into_iter().find(|model| model.id == model_id);
    let note = probe.reasoning_note.clone();
    let refreshed = store.update(|state| {
        let refreshed = {
            let saved = state.providers.iter_mut().find(|item| item.id == provider_id)
                .ok_or_else(|| AppError::ProviderNotFound(provider_id.clone()))?;
            if let Some(discovered) = discovered {
                // 只回写这一个模型，其余模型的能力与上下文窗口原样不动。
                if let Some(slot) = saved.models.iter_mut().find(|item| item.id == model_id) {
                    slot.reasoning = discovered.reasoning;
                    if discovered.context_window.is_some() { slot.context_window = discovered.context_window; }
                }
            }
            saved.clone()
        };
        refresh_current_reasoning(state, false);
        Ok(refreshed)
    })?;
    activity::record(
        "reasoning_reprobe",
        &format!("重新探测推理能力：{} / {}{}", refreshed.name, model_id, note.map(|note| format!("（{note}）")).unwrap_or_default()),
        true,
    );
    Ok(refreshed)
}

/// 汇总某个模型在某个端点上的推理档位可选面，供界面一次取齐。
///
/// **本函数不发出站请求。** 它只投影本机已有的数据：能力结论读 `ModelInfo.reasoning`，
/// 适配档位读 `AppSettings` 里用户自己写的规则表。要真的重新探测走
/// [`reprobe_model_reasoning`]（那条链路才有 Tier 0/1/2 与 TTL 退避），
/// 前端在重探成功后再调一次本命令刷新展示即可。
///
/// 往这里加 HTTP 调用会形成第二套发现逻辑：它不写 evidence、不走 TTL，
/// 迟早与 `reasoning_discovery` 的结论打架。
#[tauri::command]
fn detect_model_reasoning(
    store: State<'_, StateStore>,
    provider_id: String,
    model_id: String,
) -> AppResult<ModelReasoningMeta> {
    let state = store.read();
    let provider = state
        .providers
        .iter()
        .find(|provider| provider.id == provider_id)
        .ok_or_else(|| AppError::ProviderNotFound(provider_id.clone()))?;
    let protocol = provider.protocol;
    // 归一化失败不算错误：base_url 可能是用户刚填的草稿。此时按原样比对，
    // 最坏结果是缓存不命中、重算一次投影。
    let base_url = protocol::normalize_base_url(&provider.base_url).unwrap_or_else(|_| provider.base_url.clone());

    // 匹配档位每次实时算：用户改一条规则，缓存里的匹配结果立刻是脏的，而重算只是一次遍历。
    let matched_custom_tiers: Vec<MatchedCustomTier> = reasoning_selection::matching_custom_tiers(&model_id, &state.settings)
        .into_iter()
        .map(|hit| MatchedCustomTier {
            tier_id: hit.tier.id.clone(),
            label: hit.tier.label.clone(),
            rule_pattern: hit.rule.pattern.clone(),
            rule_match_type: hit.rule.match_type,
            supported_protocols: hit.tier.supported_protocols(),
        })
        .collect();

    if let Some(entry) = reasoning_selection::detection_cache_hit(&state.settings, &base_url, &model_id) {
        return Ok(ModelReasoningMeta {
            supported_protocols: if entry.native_param_kind == model::NativeParamKind::Unknown {
                Vec::new()
            } else {
                vec![protocol]
            },
            native_param_kind: entry.native_param_kind,
            matched_custom_tiers,
            builtin_tiers_compatible: entry.builtin_tiers_compatible,
        });
    }

    let capability = provider
        .models
        .iter()
        .find(|model| model.id == model_id)
        .and_then(|model| model.reasoning.as_ref());
    let native_param_kind = reasoning_selection::native_param_kind(capability);
    let builtin_tiers_compatible = reasoning_selection::builtin_tiers_compatible(capability);
    drop(state);

    // 回写缓存失败不该让查询失败：缓存只是省一次遍历，落盘出错时返回刚算出的投影即可。
    let _ = store.update(|state| {
        reasoning_selection::upsert_detection_cache(
            &mut state.settings,
            ReasoningDetectionCacheEntry {
                base_url: base_url.clone(),
                model_id: model_id.clone(),
                detected_at: Utc::now().to_rfc3339(),
                ttl_seconds: reasoning_capability::TTL_UNKNOWN_SECONDS,
                native_param_kind,
                builtin_tiers_compatible,
            },
        );
        Ok(())
    });

    Ok(ModelReasoningMeta {
        supported_protocols: if native_param_kind == model::NativeParamKind::Unknown {
            Vec::new()
        } else {
            vec![protocol]
        },
        native_param_kind,
        matched_custom_tiers,
        builtin_tiers_compatible,
    })
}

#[tauri::command]
async fn refresh_provider_models(store: State<'_, StateStore>, provider_id: String) -> AppResult<Provider> {
    let state = store.read();
    let provider = state.providers.iter().find(|provider| provider.id == provider_id).cloned()
        .ok_or_else(|| AppError::ProviderNotFound(provider_id.clone()))?;
    let settings = state.settings.clone();
    let api_key = credentials::get(&provider.id)?;
    let draft = provider_draft(&provider, api_key, &settings);
    match protocol::fetch_models(&draft, &settings).await {
        Ok((_target, models, confidence)) => store.update(|state| {
            let refreshed = {
                let saved = state.providers.iter_mut().find(|item| item.id == provider_id)
                    .ok_or_else(|| AppError::ProviderNotFound(provider_id.clone()))?;
                let mut models = models;
                // 这条路径只换模型列表，不动端点，所以目标就是已存的 base_url。
                let target_base_url = saved.base_url.clone();
                carry_reasoning_forward(&mut models, &saved.models, &target_base_url);
                saved.models = models;
                saved.default_model = saved.default_model.clone()
                    .filter(|model| saved.models.iter().any(|item| &item.id == model))
                    .or_else(|| saved.models.first().map(|model| model.id.clone()));
                saved.claude_model_mappings.sonnet = saved.claude_model_mappings.sonnet.clone().filter(|model| saved.models.iter().any(|item| &item.id == model));
                saved.claude_model_mappings.opus = saved.claude_model_mappings.opus.clone().filter(|model| saved.models.iter().any(|item| &item.id == model));
                saved.claude_model_mappings.haiku = saved.claude_model_mappings.haiku.clone().filter(|model| saved.models.iter().any(|item| &item.id == model));
                let models_snapshot = saved.models.clone();
                reasoning_selection::prune_missing(&mut saved.reasoning_selections, &models_snapshot);
                // 这条路径不动端点，验证记录只需按 model_id 剪枝。
                reasoning_verification::retain_for_endpoint(&mut saved.reasoning_verifications, &target_base_url, &models_snapshot);
                saved.connection_state = "connected".into();
                saved.confidence = Some(confidence);
                saved.last_checked_at = Some(Utc::now().to_rfc3339());
                saved.error_summary = None;
                saved.clone()
            };
            refresh_current_reasoning(state, true);
            Ok(refreshed)
        }),
        Err(error) => {
            let summary = error.to_string();
            store.update(|state| {
                if let Some(saved) = state.providers.iter_mut().find(|item| item.id == provider_id) {
                    saved.connection_state = "failed".into();
                    saved.error_summary = Some(summary);
                }
                Ok(())
            })?;
            Err(error)
        }
    }
}

#[tauri::command]
async fn test_provider(store: State<'_, StateStore>, provider_id: String, model_id: Option<String>) -> AppResult<ProviderTestReport> {
    let state = store.read();
    let provider = state.providers.iter().find(|provider| provider.id == provider_id).cloned()
        .ok_or_else(|| AppError::ProviderNotFound(provider_id.clone()))?;
    let settings = state.settings.clone();
    let draft = provider_draft(&provider, credentials::get(&provider.id)?, &settings);
    protocol::test_conversation(provider_id, &draft, model_id, &settings).await
}

/// 用户主动验证某个模型的某一推理档位是否真的生效。
///
/// 与 discovery 的分工：这里发一次真实请求看响应里有没有推理产物，结论落
/// `provider.reasoning_verifications`，**不碰** `model.reasoning`——包括 confidence。
/// 用户的一次成功请求不是探测事实，两条链路各自记账。
#[tauri::command]
async fn verify_model_reasoning(
    store: State<'_, StateStore>,
    provider_id: String,
    model_id: String,
    tier: reasoning_capability::ReasoningTier,
) -> AppResult<reasoning_verification::RuntimeVerification> {
    let state = store.read();
    let provider = state.providers.iter().find(|provider| provider.id == provider_id).cloned()
        .ok_or_else(|| AppError::ProviderNotFound(provider_id.clone()))?;
    let model = provider.models.iter().find(|model| model.id == model_id).cloned()
        .ok_or_else(|| AppError::InvalidInput(format!("模型 {model_id} 不属于该服务")))?;
    let capability = model.reasoning.clone()
        .ok_or_else(|| AppError::InvalidInput("该模型尚未探明推理能力，无法验证".into()))?;
    let api_key = credentials::get(&provider.id)?;

    let verification = reasoning_verification::verify_reasoning_capability(
        &provider.base_url,
        &model_id,
        &api_key,
        model.protocol,
        &capability,
        tier,
    ).await?;

    // 存储由 command 层负责：verification 模块不知道 Provider 的结构。
    // 三态一律入库——Rejected/Failed 也是用户需要看到的历史，隐藏它们等于让用户重复点。
    store.update(|state| {
        let saved = state.providers.iter_mut().find(|item| item.id == provider_id)
            .ok_or_else(|| AppError::ProviderNotFound(provider_id.clone()))?;
        saved.reasoning_verifications.entry(model_id.clone()).or_default().push(verification.clone());
        Ok(())
    })?;

    Ok(verification)
}

#[tauri::command]
fn detect_clients() -> Vec<ClientDescriptor> { clients::detect_all() }

/// 给"不需要 Provider 的启动"凑签名用的空壳。
///
/// 只有 `env_injection` 为 false 的客户端会走到这里，`env_plan` 对它们一律返回空计划，
/// 所以这里的字段值都到不了子进程。base_url 留空而不是编一个像样的地址：
/// 万一将来有人让它流到别处，空串会立刻暴露，一个假地址不会。
fn placeholder_provider() -> Provider {
    Provider {
        id: String::new(), name: String::new(), base_url: String::new(),
        protocol: model::ProtocolKind::Custom, enabled: false, is_current: false,
        default_model: None, claude_model_profile: None, claude_extended_context: false,
        claude_model_mappings: Default::default(),
        codex_compatibility: model::CodexCompatibility::NotApplicable,
        codex_probe_model: None, codex_probe_detail: None,
        reasoning_selections: Vec::new(), reasoning_verifications: Default::default(),
        models: Vec::new(), connection_state: "unknown".into(), confidence: None,
        last_checked_at: None, applied_clients: Vec::new(), error_summary: None,
    }
}

/// 启动客户端，对确认会读环境变量的客户端顺带注入密钥。
///
/// 密钥在这里现取现用：只进子进程的环境块，不落盘、不进 state.json、不进返回值。
/// base_url 的取法与 `preview_changes` 一致——ChatProxy 模式下要给本地桥的地址，
/// 否则客户端会绕过桥直连上游，而上游正是那个不支持 Responses 协议的网关。
#[tauri::command]
fn launch_client(store: State<'_, StateStore>, proxy: State<'_, LocalProxy>, client_id: String, provider_id: Option<String>) -> AppResult<launcher::LaunchOutcome> {
    let client = launcher::descriptor_for(&client_id)?;
    let state = store.read();

    // 客户端没装就别往下走：后面读凭据是有代价的动作（弹系统钥匙串授权），
    // 为一个注定失败的启动去读密钥不值得。
    if client.launch_target.is_none() {
        return Err(AppError::InvalidInput(format!("未检测到 {} 的可执行文件，请先安装该客户端", client.name)));
    }

    // 没指定 Provider（或该客户端不注入）时纯启动。空密钥不会被用到：
    // env_plan 对 env_injection 为 false 的客户端一律返回空计划。
    let Some(provider_id) = provider_id.filter(|_| client.env_injection) else {
        // 不注入的客户端（桌面端就是）压根不需要 Provider。用第一个 Provider 只是
        // 为了凑 launch 的签名；一个都没有时也要能拉起——桌面端本来就不碰 API 配置，
        // 因为"还没配服务"就拒绝启动 Claude Desktop 是没道理的。
        let provider = state.providers.first().cloned().unwrap_or_else(placeholder_provider);
        let base_url = provider.base_url.clone();
        return launcher::launch(&client, &provider, &base_url, "");
    };

    let provider = state.providers.iter().find(|item| item.id == provider_id).cloned()
        .ok_or_else(|| AppError::InvalidInput(format!("未找到该服务（{provider_id}），无法启动客户端")))?;
    // 凭据缺失要给出可操作的话术：用户需要知道去哪儿补，而不是只看到一句"读取失败"。
    let secret = credentials::get(&provider_id).map_err(|error| {
        AppError::InvalidInput(format!("未找到 {} 的 API 密钥（{error}）。请在服务编辑页重新填写并保存。", provider.name))
    })?;
    let base_url = if matches!(provider.codex_compatibility, model::CodexCompatibility::ChatProxy) {
        proxy.provider_base_url(&provider.id)
    } else {
        provider.base_url.clone()
    };
    launcher::launch(&client, &provider, &base_url, &secret)
}

#[tauri::command]
fn preview_changes(store: State<'_, StateStore>, proxy: State<'_, LocalProxy>, provider_id: String, client_ids: Vec<String>) -> AppResult<Vec<ConfigChange>> {
    let state = store.read();
    let provider = state.providers.iter().find(|provider| provider.id == provider_id).cloned().ok_or(AppError::ProviderNotFound(provider_id))?;
    if matches!(provider.codex_compatibility, model::CodexCompatibility::ChatProxy) && client_ids.iter().any(|id| id == "codex-cli") {
        let mut changes = Vec::new();
        let codex_ids = vec!["codex-cli".to_string()];
        let mut effective = provider.clone();
        effective.base_url = proxy.provider_base_url(&provider.id);
        changes.extend(config::preview(&effective, &codex_ids, &state.settings)?);
        let other_ids = client_ids.into_iter().filter(|id| id != "codex-cli").collect::<Vec<_>>();
        if !other_ids.is_empty() { changes.extend(config::preview(&provider, &other_ids, &state.settings)?); }
        Ok(changes)
    } else {
        config::preview(&provider, &client_ids, &state.settings)
    }
}

#[tauri::command]
fn apply_changes(store: State<'_, StateStore>, proxy: State<'_, LocalProxy>, provider_id: String, changes: Vec<ConfigChange>) -> AppResult<Vec<ApplyResult>> {
    let state = store.read();
    let provider = state.providers.iter().find(|provider| provider.id == provider_id).cloned().ok_or_else(|| AppError::ProviderNotFound(provider_id.clone()))?;
    let secret = credentials::get(&provider_id)?;
    let (results, backups) = if matches!(provider.codex_compatibility, model::CodexCompatibility::ChatProxy) {
        let token = credentials::proxy_token(&provider.id)?;
        proxy.register(&provider, &secret, &token, &state.settings)?;
        let mut effective = provider.clone();
        effective.base_url = proxy.provider_base_url(&provider.id);
        let codex_changes = changes.iter().filter(|change| change.client_id == "codex-cli").cloned().collect::<Vec<_>>();
        let other_changes = changes.iter().filter(|change| change.client_id != "codex-cli").cloned().collect::<Vec<_>>();
        let (mut results, mut backups) = if codex_changes.is_empty() { (Vec::new(), Vec::new()) } else { config::apply(&effective, &token, &codex_changes, &state.settings)? };
        if !other_changes.is_empty() {
            let (other_results, other_backups) = config::apply(&provider, &secret, &other_changes, &state.settings)?;
            results.extend(other_results);
            backups.extend(other_backups);
        }
        (results, backups)
    } else {
        config::apply(&provider, &secret, &changes, &state.settings)?
    };
    store.update(|state| {
        state.backups.extend(backups);
        if let Some(saved) = state.providers.iter_mut().find(|item| item.id == provider_id) {
            for result in &results { if result.success && !saved.applied_clients.contains(&result.client_id) { saved.applied_clients.push(result.client_id.clone()); } }
        }
        Ok(())
    })?;
    Ok(results)
}

#[tauri::command]
fn list_backups(store: State<'_, StateStore>) -> Vec<BackupRecord> { store.read().backups }

#[tauri::command]
fn restore_backup(store: State<'_, StateStore>, id: String) -> AppResult<()> {
    let record = store.read().backups.into_iter().find(|backup| backup.id == id).ok_or(AppError::BackupNotFound(id))?;
    config::restore_record(&record)
}

#[tauri::command]
fn get_settings(store: State<'_, StateStore>) -> AppSettings { store.read().settings }

#[tauri::command]
fn save_settings(store: State<'_, StateStore>, proxy: State<'_, LocalProxy>, mut settings: AppSettings) -> AppResult<AppSettings> {
    if settings.timeout_seconds < 3 || settings.timeout_seconds > 120 { return Err(AppError::InvalidInput("超时必须在 3 到 120 秒之间".into())); }
    settings.local_proxy_port = Some(proxy.port());
    let providers = store.read().providers;
    reasoning::refresh_settings(&mut settings, true);
    for provider in providers.iter().filter(|provider| matches!(provider.codex_compatibility, model::CodexCompatibility::ChatProxy)) {
        let api_key = credentials::get(&provider.id)?;
        let token = credentials::proxy_token(&provider.id)?;
        proxy.register(provider, &api_key, &token, &settings)?;
    }
    store.update(|state| { state.settings = settings.clone(); Ok(settings) })
}

#[tauri::command]
fn list_chat_backups(chats: State<'_, ChatStore>) -> AppResult<Vec<ChatBackupRecord>> { chats.list_backups() }

#[tauri::command]
fn chat_cache_summary(chats: State<'_, ChatStore>) -> ChatCacheSummary { chats.summary() }

#[tauri::command]
fn export_chat_backup(chats: State<'_, ChatStore>) -> AppResult<ChatBackupRecord> { chats.export_backup() }

#[tauri::command]
fn restore_chat_backup_file(chats: State<'_, ChatStore>, path: String, mode: String) -> AppResult<ChatRestoreResult> {
    Ok(chats.restore_from_file(std::path::Path::new(&path), ChatRestoreMode::parse(&mode)?))
}

#[tauri::command]
fn restore_chat_backup_payload(chats: State<'_, ChatStore>, payload: String, mode: String) -> AppResult<ChatRestoreResult> {
    Ok(chats.restore_from_payload(&payload, ChatRestoreMode::parse(&mode)?))
}

#[tauri::command]
fn restore_chat_cache(chats: State<'_, ChatStore>, mode: String) -> AppResult<ChatRestoreResult> {
    Ok(chats.restore_from_cache(ChatRestoreMode::parse(&mode)?))
}

#[tauri::command]
fn rollback_chat_restore(chats: State<'_, ChatStore>, snapshot_id: String) -> ChatRestoreResult {
    chats.rollback(&snapshot_id)
}

#[tauri::command]
fn export_providers(store: State<'_, StateStore>) -> AppResult<String> {
    serde_json::to_string_pretty(&store.read().providers).map_err(|error| AppError::Config(error.to_string()))
}

#[tauri::command]
fn import_providers(store: State<'_, StateStore>, payload: String) -> AppResult<Vec<Provider>> {
    if payload.len() > 2_000_000 { return Err(AppError::InvalidInput("导入文件过大".into())); }
    let mut providers: Vec<Provider> = serde_json::from_str(&payload).map_err(|error| AppError::InvalidInput(format!("导入 JSON 无效：{error}")))?;
    for provider in &mut providers { provider.connection_state = "untested".into(); provider.is_current = false; }
    store.update(|state| { state.providers = providers.clone(); Ok(providers) })
}

#[tauri::command]
fn diagnostics() -> HashMap<String, String> { clients::diagnostics() }

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let _ = config::repair_legacy_codex_catalog();
    let store = StateStore::load().expect("failed to initialize Provider Deck state");
    let snapshot = store.read();
    let chats = ChatStore::load().expect("failed to initialize Provider Deck chat store");
    let proxy = LocalProxy::start(snapshot.settings.local_proxy_port, chats.clone()).expect("failed to start Provider Deck local proxy");
    if snapshot.settings.local_proxy_port != Some(proxy.port()) {
        store.update(|state| { state.settings.local_proxy_port = Some(proxy.port()); Ok(()) }).expect("failed to persist local proxy port");
    }
    for provider in snapshot.providers.iter().filter(|provider| matches!(provider.codex_compatibility, model::CodexCompatibility::ChatProxy)) {
        if let (Ok(api_key), Ok(token)) = (credentials::get(&provider.id), credentials::proxy_token(&provider.id)) {
            let _ = proxy.register(provider, &api_key, &token, &snapshot.settings);
        }
    }
    tauri::Builder::default()
    .plugin(tauri_plugin_dialog::init())
    .setup(|app| {
        let open = MenuItem::with_id(app, "open", "打开 Provider Deck", true, None::<&str>)?;
        let hide = MenuItem::with_id(app, "hide", "隐藏窗口", true, None::<&str>)?;
        let quit = MenuItem::with_id(app, "quit", "退出程序", true, None::<&str>)?;
        let menu = Menu::with_items(app, &[&open, &hide, &quit])?;
        let mut tray = TrayIconBuilder::new().menu(&menu).show_menu_on_left_click(false);
        if let Some(icon) = app.default_window_icon() { tray = tray.icon(icon.clone()); }
        tray.on_menu_event(|app, event| match event.id.as_ref() {
            "open" => {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.unminimize();
                    let _ = window.set_focus();
                }
            }
            "hide" => {
                if let Some(window) = app.get_webview_window("main") { let _ = window.hide(); }
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                if let Some(window) = tray.app_handle().get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.unminimize();
                    let _ = window.set_focus();
                }
            }
        })
        .build(app)?;
        Ok(())
    })
    .on_window_event(|window, event| {
        if let WindowEvent::CloseRequested { api, .. } = event {
            api.prevent_close();
            let _ = window.hide();
        }
    })
    .manage(store).manage(proxy).manage(chats).invoke_handler(tauri::generate_handler![
        list_providers, get_provider_api_key, save_provider, delete_provider, set_current_provider, probe_provider, reprobe_provider, detect_clients, launch_client,
        refresh_provider_models, reprobe_model_reasoning, detect_model_reasoning, verify_model_reasoning, test_provider,
        preview_changes, apply_changes, list_backups, restore_backup, get_settings, save_settings,
        list_chat_backups, chat_cache_summary, export_chat_backup, restore_chat_backup_file, restore_chat_backup_payload, restore_chat_cache, rollback_chat_restore,
        export_providers, import_providers, diagnostics
    ]).run(tauri::generate_context!()).expect("error while running Provider Deck");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reasoning_capability::{ReasoningCapability, ReasoningConfidence, ReasoningKey};

    fn model(id: &str, reasoning: Option<ReasoningCapability>) -> model::ModelInfo {
        model::ModelInfo {
            id: id.into(),
            display_name: id.into(),
            provider: None,
            protocol: model::ProtocolKind::Openai,
            source: "test".into(),
            capabilities: Vec::new(),
            context_window: None,
            parameter_count_billions: None,
            reasoning,
        }
    }

    fn capability(base_url: &str, model_id: &str) -> ReasoningCapability {
        ReasoningCapability::from_effort_enum(
            ReasoningKey::new(base_url, model_id),
            &["low".into(), "high".into()],
            ReasoningConfidence::Declared,
        )
    }

    /// 占位 Provider 不能带任何能流到子进程的内容。
    ///
    /// 它只在"不注入的客户端 + 用户还没配服务"这条路径上出现。今天 env_plan 对
    /// 这些客户端返回空计划，所以字段值到不了子进程；这个测试钉住的是**将来**
    /// 有人改了 env_plan 时，占位值本身也是无害的。
    #[test]
    fn placeholder_provider_carries_nothing_injectable() {
        let placeholder = placeholder_provider();
        assert!(placeholder.base_url.is_empty(), "占位 Provider 不得带 base_url");
        assert!(placeholder.id.is_empty());
        assert!(placeholder.default_model.is_none());
        assert!(!placeholder.enabled, "占位 Provider 不得是启用状态");
        // 空计划是前提：任何客户端配上这个占位 Provider 都不该产出注入项。
        for id in ["claude-desktop", "chatgpt-desktop", "codex-cli", "claude-code"] {
            let client = launcher::descriptor_for(id).expect("客户端未注册");
            assert!(launcher::env_plan(&client, &placeholder, "", "").is_empty(), "{id} 对占位 Provider 产出了注入项");
        }
    }

    /// 要求 5：刷新 / 重新检测模型不能丢掉已发现的推理能力。
    /// 丢了的话 Unknown 的 6 小时退避窗口会归零，用户每次"获取模型"都会触发重探。
    #[test]
    fn refresh_carries_reasoning_forward() {
        let previous = vec![
            model("gpt-x", Some(capability("https://api.example.com/v1", "gpt-x"))),
            model("gpt-y", Some(capability("https://api.example.com/v1", "gpt-y"))),
        ];
        // 刷新拿到的新列表：能力字段一律为空，且模型集合有增有减。
        let mut fresh = vec![model("gpt-x", None), model("gpt-z", None)];

        carry_reasoning_forward(&mut fresh, &previous, "https://api.example.com/v1");

        assert!(fresh[0].reasoning.is_some(), "已发现的能力在刷新后丢失");
        assert_eq!(fresh[0].reasoning.as_ref().unwrap().tiers.len(), 2);
        assert!(fresh[1].reasoning.is_none(), "新模型不应继承其他模型的能力");
    }

    /// P0-1：换 base_url 后旧能力不得被迁移到新列表。
    ///
    /// 迁移按 model_id 对齐，但能力归属 (base_url, model_id)。编排器只对"被选中的
    /// 那一个模型"跑发现，其余模型不会经过 key 校验——旧键就这样留在了新 Provider 上，
    /// 前端拿到的是属于另一个 base_url 的档位表。
    #[test]
    fn reasoning_does_not_carry_across_base_urls() {
        let previous = vec![model("gpt-x", Some(capability("https://old.example.com/v1", "gpt-x")))];
        let mut fresh = vec![model("gpt-x", None)];
        carry_reasoning_forward(&mut fresh, &previous, "https://new.example.com/v1");
        let carried = fresh[0].reasoning.as_ref().map(|item| item.key.base_url.clone());
        assert_eq!(carried, None, "跨 base_url 迁移了旧能力，实际带过来的键是 {carried:?}");
    }

    /// P0-1 的反向约束：同端点必须照常继承。
    ///
    /// 把 key 校验写成"一律不继承"也能让上一个测试变绿，但那样每次刷新都清空缓存，
    /// 等于把正确性问题换成"每次保存重发一轮 Tier 2 探测"的成本问题。
    #[test]
    fn same_base_url_still_inherits_capability() {
        let previous = vec![model("gpt-x", Some(capability("https://api.example.com/v1", "gpt-x")))];
        let mut fresh = vec![model("gpt-x", None)];
        carry_reasoning_forward(&mut fresh, &previous, "https://api.example.com/v1");
        assert!(fresh[0].reasoning.is_some(), "同端点的能力缓存被误删，会导致每次刷新都重探");
    }

    /// 迁移必须拿**归一化后**的 base_url 判定。
    ///
    /// 能力键里存的是 `normalized_base_url`；若调用方传用户原始输入（尾斜杠、缺 /v1、
    /// 大小写），每次保存都会失配，缓存永远命中不了。
    #[test]
    fn migration_judges_against_normalized_base_url() {
        let previous = vec![model("gpt-x", Some(capability("https://api.example.com/v1", "gpt-x")))];

        let mut fresh = vec![model("gpt-x", None)];
        carry_reasoning_forward(&mut fresh, &previous, "https://api.example.com/v1/");
        assert!(fresh[0].reasoning.is_none(), "带尾斜杠的原始输入不应被当成同一端点");

        let mut fresh = vec![model("gpt-x", None)];
        carry_reasoning_forward(&mut fresh, &previous, "https://api.example.com/v1");
        assert!(fresh[0].reasoning.is_some(), "归一化后的值必须命中缓存");
    }

    /// capability / confidence / evidence 三者同生同死，不做部分迁移。
    #[test]
    fn foreign_capability_is_discarded_whole() {
        let mut stale = capability("https://old.example.com/v1", "gpt-x");
        stale.confidence = ReasoningConfidence::Verified;
        stale.push_evidence(crate::reasoning_capability::ReasoningEvidence::new(
            crate::reasoning_capability::EvidenceSource::CapabilityValidation,
            Some("https://old.example.com/v1/chat/completions".into()),
            "旧端点上确证过",
        ));
        let previous = vec![model("gpt-x", Some(stale))];
        let mut fresh = vec![model("gpt-x", None)];

        carry_reasoning_forward(&mut fresh, &previous, "https://new.example.com/v1");

        assert!(fresh[0].reasoning.is_none(), "留下 evidence 却换掉端点，等于用旧证据为新端点背书");
    }

    /// 探测本轮已产出的能力优先，不被旧缓存覆盖。
    #[test]
    fn fresh_reasoning_wins_over_previous() {
        let previous = vec![model("gpt-x", Some(capability("https://api.example.com/v1", "gpt-x")))];
        let mut discovered = capability("https://api.example.com/v1", "gpt-x");
        discovered.confidence = ReasoningConfidence::Validated;
        let mut fresh = vec![model("gpt-x", Some(discovered))];

        carry_reasoning_forward(&mut fresh, &previous, "https://api.example.com/v1");

        assert_eq!(fresh[0].reasoning.as_ref().unwrap().confidence, ReasoningConfidence::Validated);
    }

    /// 用户选择跨端点保留，只按 model_id 剪枝。
    ///
    /// 与上面 capability 的严格 key 校验刻意不对称：能力是事实（换端点即失效），
    /// 选择是意图（换端点依然成立）。
    #[test]
    fn selections_survive_base_url_change_but_drop_vanished_models() {
        use crate::reasoning_capability::ReasoningTier;
        use crate::reasoning_selection::{prune_missing, ReasoningSelection, SelectionSource};

        let mut selections = vec![
            ReasoningSelection::new("gpt-x", ReasoningTier::Deep, SelectionSource::User),
            ReasoningSelection::new("gpt-gone", ReasoningTier::Light, SelectionSource::User),
        ];
        // 新端点、新模型列表：gpt-x 还在，gpt-gone 消失了。
        let models = vec![model("gpt-x", None), model("gpt-new", None)];

        prune_missing(&mut selections, &models);

        assert_eq!(selections.len(), 1);
        assert_eq!(selections[0].model_id, "gpt-x");
        assert_eq!(selections[0].tier, Some(ReasoningTier::Deep), "换端点不应改变用户意图");
    }

    /// 保存时草稿只提交正在编辑的那个模型，其他模型的选择不能被清掉。
    #[test]
    fn saving_one_model_keeps_other_selections() {
        use crate::reasoning_capability::ReasoningTier;
        use crate::reasoning_selection::{merge_drafted, prune_missing, ReasoningSelection, SelectionSource};

        let existing = vec![
            ReasoningSelection::new("gpt-x", ReasoningTier::Light, SelectionSource::User),
            ReasoningSelection::new("gpt-y", ReasoningTier::Deep, SelectionSource::User),
        ];
        let drafted = vec![ReasoningSelection::new("gpt-x", ReasoningTier::Max, SelectionSource::User)];

        let mut merged = merge_drafted(&existing, &drafted);
        prune_missing(&mut merged, &[model("gpt-x", None), model("gpt-y", None)]);

        assert_eq!(merged.len(), 2, "草稿未提到的模型被误删");
        let tier_of = |id: &str| merged.iter().find(|item| item.model_id == id).and_then(|item| item.tier);
        assert_eq!(tier_of("gpt-x"), Some(ReasoningTier::Max), "草稿应覆盖同模型的旧选择");
        assert_eq!(tier_of("gpt-y"), Some(ReasoningTier::Deep));
    }
}
