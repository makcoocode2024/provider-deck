use std::{env, path::PathBuf};
use directories::UserDirs;
use crate::model::{ClientDescriptor, ProtocolKind};

fn platform() -> &'static str {
    if cfg!(windows) { "windows" } else if cfg!(target_os = "macos") { "macos" } else { "linux" }
}

pub fn config_path(client_id: &str) -> Option<PathBuf> {
    let home = UserDirs::new()?.home_dir().to_path_buf();
    match client_id {
        "codex-cli" => Some(home.join(".codex").join("config.toml")),
        "claude-code" => Some(home.join(".claude").join("settings.json")),
        "gemini-cli" => Some(home.join(".gemini").join("settings.json")),
        "opencode" => Some(home.join(".config").join("opencode").join("opencode.json")),
        _ => None,
    }
}

fn find_command(names: &[&str]) -> Option<String> {
    names.iter().find_map(|name| which::which(name).ok()).map(|path| path.to_string_lossy().into_owned())
}

fn descriptor(id: &str, name: &str, commands: &[&str], protocols: Vec<ProtocolKind>, support: &str, auto_config: bool, guidance: &str) -> ClientDescriptor {
    let detected_path = find_command(commands);
    ClientDescriptor {
        id: id.into(), name: name.into(), platforms: vec![platform().into()], protocols,
        installed: detected_path.is_some(), detected_path, config_path: config_path(id).map(|p| p.to_string_lossy().into_owned()),
        support: support.into(), auto_config, requires_restart: true, guidance: guidance.into(),
    }
}

pub fn detect_all() -> Vec<ClientDescriptor> {
    let all = vec![ProtocolKind::Openai, ProtocolKind::Anthropic, ProtocolKind::Gemini, ProtocolKind::AzureOpenai, ProtocolKind::Custom];
    vec![
        descriptor("codex-cli", "OpenAI Codex CLI", &["codex"], vec![ProtocolKind::Openai, ProtocolKind::AzureOpenai], "verified", true, "使用公开 TOML model_providers 结构。"),
        descriptor("claude-code", "Claude Code", &["claude"], vec![ProtocolKind::Anthropic], "verified", true, "合并 ~/.claude/settings.json 的 env 字段。"),
        descriptor("gemini-cli", "Gemini CLI", &["gemini"], vec![ProtocolKind::Gemini], "experimental", false, "官方已公告迁移安排，仅生成手动配置说明。"),
        descriptor("opencode", "OpenCode", &["opencode"], all.clone(), "verified", true, "使用官方 JSON Provider 结构。"),
        descriptor("vs-code", "VS Code", &["code"], all.clone(), "manual", false, "仅检测安装状态，不修改扩展内部数据。"),
        descriptor("cursor", "Cursor", &["cursor"], all.clone(), "manual", false, "仅检测安装状态，不修改内部数据库。"),
        descriptor("windsurf", "Windsurf", &["windsurf"], all.clone(), "manual", false, "仅检测安装状态，不修改内部数据库。"),
        descriptor("cline", "Cline", &[], all.clone(), "manual", false, "请在扩展设置中手动配置。"),
        descriptor("roo-code", "Roo Code", &[], all.clone(), "manual", false, "请在扩展设置中手动配置。"),
        descriptor("continue", "Continue", &[], all, "manual", false, "请按 Continue 当前公开文档手动配置。"),
    ]
}

pub fn diagnostics() -> std::collections::HashMap<String, String> {
    let mut result = std::collections::HashMap::new();
    result.insert("platform".into(), platform().into());
    result.insert("architecture".into(), env::consts::ARCH.into());
    result.insert("version".into(), env!("CARGO_PKG_VERSION").into());
    result.insert("credentialStore".into(), if cfg!(windows) { "Windows Credential Manager" } else if cfg!(target_os = "macos") { "macOS Keychain" } else { "Secret Service" }.into());
    for client in detect_all() {
        result.insert(format!("client.{}", client.id), if client.installed { "已安装" } else { "未检测到" }.into());
    }
    result
}
