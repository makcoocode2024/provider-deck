mod clients;
mod config;
mod credentials;
mod error;
mod model;
mod local_proxy;
mod responses_chat;
mod protocol;
mod redaction;
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
use model::{AppSettings, ApplyResult, BackupRecord, ClientDescriptor, ConfigChange, ProbeResult, Provider, ProviderDraft, ProviderTestReport};
use local_proxy::LocalProxy;
use storage::StateStore;

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
            models: probe.models.clone(), connection_state: "connected".into(), confidence: Some(probe.confidence),
            last_checked_at: Some(Utc::now().to_rfc3339()), applied_clients: existing.map(|p| p.applied_clients).unwrap_or_default(), error_summary: None,
        };
        state.providers.retain(|item| item.id != id);
        state.providers.push(provider.clone());
        Ok(provider)
    })?;
    if matches!(provider.codex_compatibility, model::CodexCompatibility::ChatProxy) {
        let token = credentials::proxy_token(&provider.id)?;
        proxy.register(&provider, &resolved_draft.api_key, &token, &settings)?;
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
fn set_current_provider(store: State<'_, StateStore>, id: String) -> AppResult<Vec<Provider>> {
    store.update(|state| {
        if !state.providers.iter().any(|provider| provider.id == id) { return Err(AppError::ProviderNotFound(id)); }
        for provider in &mut state.providers { provider.is_current = provider.id == id; }
        Ok(state.providers.clone())
    })
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
    };

    match protocol::probe(&draft, &settings).await {
        Ok(probe) => {
            let refreshed = store.update(|state| {
            let saved = state.providers.iter_mut().find(|item| item.id == id)
                .ok_or_else(|| AppError::ProviderNotFound(id.clone()))?;
            saved.base_url = probe.normalized_base_url;
            saved.protocol = probe.protocol;
            saved.models = probe.models;
            saved.codex_compatibility = probe.codex_compatibility;
            saved.codex_probe_model = probe.codex_probe_model;
            saved.codex_probe_detail = probe.codex_probe_detail;
            saved.default_model = saved.default_model.clone().filter(|model| saved.models.iter().any(|item| &item.id == model)).or_else(|| saved.models.first().map(|model| model.id.clone()));
            saved.claude_model_mappings.sonnet = saved.claude_model_mappings.sonnet.clone().filter(|model| saved.models.iter().any(|item| &item.id == model));
            saved.claude_model_mappings.opus = saved.claude_model_mappings.opus.clone().filter(|model| saved.models.iter().any(|item| &item.id == model));
            saved.claude_model_mappings.haiku = saved.claude_model_mappings.haiku.clone().filter(|model| saved.models.iter().any(|item| &item.id == model));
            saved.connection_state = "connected".into();
            saved.confidence = Some(probe.confidence);
            saved.last_checked_at = Some(Utc::now().to_rfc3339());
            saved.error_summary = None;
            Ok(saved.clone())
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
    }
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
            let saved = state.providers.iter_mut().find(|item| item.id == provider_id)
                .ok_or_else(|| AppError::ProviderNotFound(provider_id.clone()))?;
            saved.models = models;
            saved.default_model = saved.default_model.clone()
                .filter(|model| saved.models.iter().any(|item| &item.id == model))
                .or_else(|| saved.models.first().map(|model| model.id.clone()));
            saved.claude_model_mappings.sonnet = saved.claude_model_mappings.sonnet.clone().filter(|model| saved.models.iter().any(|item| &item.id == model));
            saved.claude_model_mappings.opus = saved.claude_model_mappings.opus.clone().filter(|model| saved.models.iter().any(|item| &item.id == model));
            saved.claude_model_mappings.haiku = saved.claude_model_mappings.haiku.clone().filter(|model| saved.models.iter().any(|item| &item.id == model));
            saved.connection_state = "connected".into();
            saved.confidence = Some(confidence);
            saved.last_checked_at = Some(Utc::now().to_rfc3339());
            saved.error_summary = None;
            Ok(saved.clone())
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

#[tauri::command]
fn detect_clients() -> Vec<ClientDescriptor> { clients::detect_all() }

#[tauri::command]
fn preview_changes(store: State<'_, StateStore>, proxy: State<'_, LocalProxy>, provider_id: String, client_ids: Vec<String>) -> AppResult<Vec<ConfigChange>> {
    let provider = store.read().providers.into_iter().find(|provider| provider.id == provider_id).ok_or(AppError::ProviderNotFound(provider_id))?;
    if matches!(provider.codex_compatibility, model::CodexCompatibility::ChatProxy) && client_ids.iter().any(|id| id == "codex-cli") {
        let mut changes = Vec::new();
        let codex_ids = vec!["codex-cli".to_string()];
        let mut effective = provider.clone();
        effective.base_url = proxy.provider_base_url(&provider.id);
        changes.extend(config::preview(&effective, &codex_ids)?);
        let other_ids = client_ids.into_iter().filter(|id| id != "codex-cli").collect::<Vec<_>>();
        if !other_ids.is_empty() { changes.extend(config::preview(&provider, &other_ids)?); }
        Ok(changes)
    } else {
        config::preview(&provider, &client_ids)
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
fn save_settings(store: State<'_, StateStore>, proxy: State<'_, LocalProxy>, mut settings: AppSettings) -> AppResult<()> {
    if settings.timeout_seconds < 3 || settings.timeout_seconds > 120 { return Err(AppError::InvalidInput("超时必须在 3 到 120 秒之间".into())); }
    settings.local_proxy_port = Some(proxy.port());
    for provider in store.read().providers.iter().filter(|provider| matches!(provider.codex_compatibility, model::CodexCompatibility::ChatProxy)) {
        let api_key = credentials::get(&provider.id)?;
        let token = credentials::proxy_token(&provider.id)?;
        proxy.register(provider, &api_key, &token, &settings)?;
    }
    store.update(|state| { state.settings = settings; Ok(()) })
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
    let proxy = LocalProxy::start(snapshot.settings.local_proxy_port).expect("failed to start Provider Deck local proxy");
    if snapshot.settings.local_proxy_port != Some(proxy.port()) {
        store.update(|state| { state.settings.local_proxy_port = Some(proxy.port()); Ok(()) }).expect("failed to persist local proxy port");
    }
    for provider in snapshot.providers.iter().filter(|provider| matches!(provider.codex_compatibility, model::CodexCompatibility::ChatProxy)) {
        if let (Ok(api_key), Ok(token)) = (credentials::get(&provider.id), credentials::proxy_token(&provider.id)) {
            let _ = proxy.register(provider, &api_key, &token, &snapshot.settings);
        }
    }
    tauri::Builder::default()
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
    .manage(store).manage(proxy).invoke_handler(tauri::generate_handler![
        list_providers, get_provider_api_key, save_provider, delete_provider, set_current_provider, probe_provider, reprobe_provider, detect_clients,
        refresh_provider_models, test_provider,
        preview_changes, apply_changes, list_backups, restore_backup, get_settings, save_settings,
        export_providers, import_providers, diagnostics
    ]).run(tauri::generate_context!()).expect("error while running Provider Deck");
}
