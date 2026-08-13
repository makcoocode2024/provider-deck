//! 启动第三方客户端，可选地把 API Key 通过环境变量注入子进程。
//!
//! 与配置写入的分工：`config.rs` 把密钥写进客户端的配置文件（落盘、长期存在），
//! 这里把密钥只放进子进程的环境块（随进程结束消失，不落盘）。对确认会读环境变量的
//! 客户端，后者能让配置文件里一个明文密钥都不留。
//!
//! 三条不做的事：
//! - 不修改客户端安装包，只 spawn 已经装好的可执行文件
//! - 不做进程注入，不碰目标进程的内存
//! - 不读写客户端的内部数据库或会话存储
//!
//! 环境变量注入的代价必须讲清楚：子进程的环境块对**同一用户的任何进程**可读
//! （Process Explorer 即可）。这比配置文件明文强——不落盘、不进备份、不进导出——
//! 但不是保密。`LaunchOutcome.warnings` 会把这句话带到界面上。

use std::collections::BTreeMap;
use std::process::Command;

use serde::{Deserialize, Serialize};

use crate::clients;
use crate::error::{AppError, AppResult};
use crate::model::{ClientDescriptor, Provider, ProtocolKind, SupportLevel};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LaunchOutcome {
    pub client_id: String,
    pub client_name: String,
    /// 实际启动的可执行文件。
    pub launched_path: String,
    /// 本次注入的环境变量名。**只有名字，永远没有值**——
    /// 这个结构体会进入界面、日志和诊断，带上值就等于把密钥泄进这三处。
    pub injected_variables: Vec<String>,
    pub warnings: Vec<String>,
}

/// 供注入的环境变量。
///
/// 用 BTreeMap 而不是 HashMap：注入顺序影响不到子进程，但影响 `injected_variables`
/// 的顺序，进而影响测试和界面文案的稳定性。
type EnvPlan = BTreeMap<String, String>;

/// Anthropic 系客户端的环境变量。
///
/// 这三个名字与 `config::merge_claude` 写进 settings.json 的完全一致——那边已经核实过
/// Claude Code 会读它们。两处必须同源：如果这里多编一个变量名，注入了也没人读，
/// 用户会以为配置生效了而实际没有。
fn anthropic_plan(base_url: &str, secret: &str) -> EnvPlan {
    let mut plan = EnvPlan::new();
    plan.insert("ANTHROPIC_BASE_URL".into(), base_url.to_owned());
    plan.insert("ANTHROPIC_API_KEY".into(), secret.to_owned());
    plan.insert("CLAUDE_CODE_ENABLE_GATEWAY_MODEL_DISCOVERY".into(), "0".into());
    plan
}

/// Codex CLI 的环境变量。
///
/// 只注入密钥，不注入 base_url：Codex 的 base_url 来自 `config.toml` 的
/// `model_providers.<id>.base_url`，没有对应的环境变量入口。变量名必须与
/// `config::CODEX_ENV_KEY` 写进 `env_key` 的那个字面量一致——本机 0.147.0 实测：
/// `env_key` 指向的变量未设置时直接报错退出，不会回落到 `experimental_bearer_token`。
fn openai_plan(secret: &str) -> EnvPlan {
    let mut plan = EnvPlan::new();
    plan.insert(crate::config::CODEX_ENV_KEY.into(), secret.to_owned());
    plan
}

/// 按客户端和协议决定注入什么。
///
/// 返回空表示"这个客户端没有已核实的环境变量入口"——此时只启动，不注入。
/// 不做兜底猜测：注入一个没人读的变量名，只会白担一份暴露风险。
pub fn env_plan(client: &ClientDescriptor, provider: &Provider, base_url: &str, secret: &str) -> EnvPlan {
    if !client.env_injection { return EnvPlan::new(); }
    match (client.id.as_str(), &provider.protocol) {
        ("claude-code", ProtocolKind::Anthropic) => anthropic_plan(base_url, secret),
        // Azure 也走这条：config::merge_codex 对两种协议写的是同一套 model_providers 结构。
        ("codex-cli", ProtocolKind::Openai | ProtocolKind::AzureOpenai) => openai_plan(secret),
        _ => EnvPlan::new(),
    }
}

fn warnings_for(client: &ClientDescriptor, injected: bool) -> Vec<String> {
    let mut warnings = Vec::new();
    if injected {
        warnings.push("API Key 已通过环境变量注入本次启动的进程，不写入任何配置文件，进程退出即消失。".into());
        warnings.push("注意：子进程的环境变量对当前用户的其他进程可见（例如任务管理器或 Process Explorer）。这避免了配置文件明文，但不等于加密保护。".into());
        if client.id == "codex-cli" {
            warnings.push("Codex 采用环境变量鉴权，配置文件内没有明文密钥。因此独立终端手动执行 codex 会提示环境变量缺失，请从本工具启动。".into());
        }
    } else if client.auto_config {
        warnings.push("该客户端未确认支持从环境变量读取密钥，本次仅启动进程，密钥仍按既有配置文件方式使用。".into());
    } else {
        warnings.push("该客户端仅启动，不注入任何 API 配置。它按账号登录，Provider Deck 不读写其内部会话数据。".into());
    }
    if client.requires_restart {
        warnings.push("若该客户端已在运行，新的环境变量不会影响已存在的进程，需要先完全退出再启动。".into());
    }
    warnings
}

/// 启动客户端。
///
/// `secret` 由 command 层从系统凭据库现取，用完即随子进程环境消失，不在本模块留存。
pub fn launch(client: &ClientDescriptor, provider: &Provider, base_url: &str, secret: &str) -> AppResult<LaunchOutcome> {
    let target = client.launch_target.clone().ok_or_else(|| {
        AppError::InvalidInput(format!("未检测到 {} 的可执行文件，无法启动", client.name))
    })?;
    let plan = env_plan(client, provider, base_url, secret);

    // 用参数数组而不是拼命令行字符串：路径里的空格和引号在字符串拼接下会变成
    // 额外的参数或者注入点。Command 直接把可执行文件路径当单个参数传给系统调用。
    let mut command = Command::new(&target);
    for (key, value) in &plan {
        command.env(key, value);
    }
    command.spawn().map_err(|error| {
        AppError::Config(format!("启动 {} 失败：{error}", client.name))
    })?;

    Ok(LaunchOutcome {
        client_id: client.id.clone(),
        client_name: client.name.clone(),
        launched_path: target,
        injected_variables: plan.keys().cloned().collect(),
        warnings: warnings_for(client, !plan.is_empty()),
    })
}

pub fn descriptor_for(client_id: &str) -> AppResult<ClientDescriptor> {
    clients::detect_all().into_iter().find(|item| item.id == client_id)
        .ok_or_else(|| AppError::InvalidInput(format!("未知客户端：{client_id}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ClaudeModelMappings, CodexCompatibility};

    fn client(id: &str, env_injection: bool, auto_config: bool) -> ClientDescriptor {
        ClientDescriptor {
            id: id.into(), name: format!("客户端 {id}"), platforms: vec!["windows".into()],
            protocols: vec![ProtocolKind::Anthropic], installed: true,
            detected_path: Some("C:\\Tools\\x.exe".into()), config_path: None,
            support: SupportLevel::Verified, auto_config, requires_restart: true,
            guidance: String::new(), launch_target: Some("C:\\Tools\\x.exe".into()), env_injection,
        }
    }

    fn provider(protocol: ProtocolKind) -> Provider {
        Provider {
            id: "p1".into(), name: "网关".into(), base_url: "https://gw.example.com/v1".into(),
            protocol, enabled: true, is_current: true, default_model: Some("m1".into()),
            claude_model_profile: None, claude_extended_context: false,
            claude_model_mappings: ClaudeModelMappings::default(),
            codex_compatibility: CodexCompatibility::Unknown,
            codex_probe_model: None, codex_probe_detail: None,
            reasoning_selections: Vec::new(), reasoning_verifications: Default::default(),
            models: Vec::new(), connection_state: "ok".into(), confidence: None,
            last_checked_at: None, applied_clients: Vec::new(), error_summary: None,
        }
    }

    /// LaunchOutcome 会进界面、日志和诊断。它只能带变量名，不能带值——
    /// 带上值就等于把密钥泄进这三处，等同于配置文件明文但更难发现。
    #[test]
    fn launch_outcome_carries_names_but_never_values() {
        let secret = "sk-must-not-appear-anywhere";
        let plan = env_plan(&client("claude-code", true, true), &provider(ProtocolKind::Anthropic), "https://gw.example.com", secret);
        let outcome = LaunchOutcome {
            client_id: "claude-code".into(), client_name: "Claude Code".into(),
            launched_path: "C:\\Tools\\claude.exe".into(),
            injected_variables: plan.keys().cloned().collect(),
            warnings: warnings_for(&client("claude-code", true, true), true),
        };
        let json = serde_json::to_string(&outcome).expect("序列化失败");
        assert!(!json.contains(secret), "LaunchOutcome 泄露了密钥值");
        assert!(outcome.injected_variables.iter().any(|name| name == "ANTHROPIC_API_KEY"));
    }

    /// 只有确认会读环境变量的客户端才注入。其余一律空计划：
    /// 注入没人读的变量名不会让配置生效，只会白担一份暴露风险。
    #[test]
    fn injection_only_for_verified_env_readers() {
        let p = provider(ProtocolKind::Anthropic);
        assert!(!env_plan(&client("claude-code", true, true), &p, "https://x", "s").is_empty());
        assert!(env_plan(&client("claude-code", false, true), &p, "https://x", "s").is_empty(), "env_injection 为 false 时不得注入");
        // codex-cli 现在有已核实的入口了（config::CODEX_ENV_KEY 写进 env_key），
        // 但只在 OpenAI 系协议下——这里的 p 是 Anthropic，所以仍然为空。
        assert!(env_plan(&client("codex-cli", true, true), &p, "https://x", "s").is_empty(), "协议不匹配时 codex-cli 不得注入");
        assert!(!env_plan(&client("codex-cli", true, true), &provider(ProtocolKind::Openai), "https://x", "s").is_empty(), "OpenAI 协议下 codex-cli 应当注入");
        assert!(env_plan(&client("codex-cli", false, true), &provider(ProtocolKind::Openai), "https://x", "s").is_empty(), "env_injection 为 false 时不得注入");
        assert!(env_plan(&client("claude-desktop", true, false), &p, "https://x", "s").is_empty(), "桌面客户端不得注入");
        assert!(env_plan(&client("claude-desktop", true, false), &provider(ProtocolKind::Openai), "https://x", "s").is_empty(), "桌面客户端在任何协议下都不得注入");
    }

    /// 注入的变量名必须与 config::merge_codex 写进 env_key 的完全一致。
    ///
    /// 这是本次改造最容易悄悄坏掉的地方：env_key 是硬要求，两边对不上时
    /// Codex 会直接报 "Missing environment variable" 退出，而不是回落到别的鉴权方式。
    #[test]
    fn codex_variable_name_matches_the_config_writer() {
        let plan = openai_plan("s");
        let names: Vec<&str> = plan.keys().map(String::as_str).collect();
        assert_eq!(names, vec![crate::config::CODEX_ENV_KEY], "注入的变量名必须只有 env_key 那一个");

        // 反向核对留在 config.rs 的 env_key_is_written_and_matches_the_launcher 里，
        // 那边能直接调 merge_codex 看写出的 TOML。这里只钉住"注入的就是这个常量"。
    }

    /// 桌面客户端启动时环境里不能出现任何密钥变量。
    #[test]
    fn desktop_launch_carries_no_secret_variables() {
        let secret = "sk-desktop-must-never-see-this";
        for id in ["claude-desktop", "chatgpt-desktop"] {
            for protocol in [ProtocolKind::Anthropic, ProtocolKind::Openai, ProtocolKind::Gemini] {
                let plan = env_plan(&client(id, false, false), &provider(protocol), "https://x", secret);
                assert!(plan.is_empty(), "{id} 的启动计划里出现了环境变量");
                assert!(!plan.values().any(|v| v.contains(secret)), "{id} 的启动计划带上了密钥值");
            }
        }
    }

    /// 错误信息里不能出现密钥的任何片段。
    ///
    /// 启动失败的报错会进 toast、日志和诊断包。把密钥拼进去就等于在三处泄密，
    /// 而且比配置文件明文更难发现——没人会去审计错误文案。
    #[test]
    fn launch_errors_never_leak_the_secret() {
        let secret = "sk-error-path-must-not-leak";
        let mut target = client("codex-cli", true, true);
        target.launch_target = None;
        let missing = launch(&target, &provider(ProtocolKind::Openai), "https://x", secret).expect_err("应当报错");
        assert!(!missing.to_string().contains(secret), "未安装的报错泄露了密钥");

        let mut broken = client("codex-cli", true, true);
        broken.launch_target = Some("C:\\Nonexistent\\provider-deck-no-such-binary.exe".into());
        let failed = launch(&broken, &provider(ProtocolKind::Openai), "https://x", secret).expect_err("应当报错");
        assert!(!failed.to_string().contains(secret), "启动失败的报错泄露了密钥");
        assert!(failed.to_string().contains("启动"), "启动失败的报错缺少中文说明");
    }

    /// codex-cli 注入时必须说明"独立终端会提示环境变量缺失"。
    ///
    /// 这是方案 B 的代价，用户必须在界面上看到，否则会以为是程序坏了。
    #[test]
    fn codex_warns_that_standalone_terminal_will_fail() {
        let warnings = warnings_for(&client("codex-cli", true, true), true);
        assert!(warnings.iter().any(|item| item.contains("独立终端")), "缺少独立终端失效提醒");
        assert!(warnings.iter().any(|item| item.contains("配置文件内没有明文密钥")), "缺少无明文声明");
        // claude-code 不该带上这句——它的 settings.json 里仍然有既有配置路径。
        let claude = warnings_for(&client("claude-code", true, true), true);
        assert!(!claude.iter().any(|item| item.contains("独立终端")), "这句只适用于 codex-cli");
    }

    /// 协议不匹配时不注入：给 OpenAI 协议的 Provider 发 ANTHROPIC_* 会让
    /// 客户端拿着错误的端点去请求，比不注入更难排查。
    #[test]
    fn injection_requires_matching_protocol() {
        let target = client("claude-code", true, true);
        assert!(env_plan(&target, &provider(ProtocolKind::Openai), "https://x", "s").is_empty());
        assert!(env_plan(&target, &provider(ProtocolKind::Gemini), "https://x", "s").is_empty());
        assert!(!env_plan(&target, &provider(ProtocolKind::Anthropic), "https://x", "s").is_empty());
    }

    /// 注入的变量名必须与 config::merge_claude 写进 settings.json 的一致。
    /// 两处同源，否则注入了也没人读。
    #[test]
    fn anthropic_variable_names_match_the_config_writer() {
        let plan = anthropic_plan("https://gw.example.com", "s");
        let names: Vec<&str> = plan.keys().map(String::as_str).collect();
        assert_eq!(names, vec!["ANTHROPIC_API_KEY", "ANTHROPIC_BASE_URL", "CLAUDE_CODE_ENABLE_GATEWAY_MODEL_DISCOVERY"]);
        assert_eq!(plan.get("ANTHROPIC_BASE_URL").map(String::as_str), Some("https://gw.example.com"));
    }

    /// 注入时必须同时给出"环境变量对同用户进程可见"这条提醒。
    /// 用户需要知道这是"不落盘"而不是"加密"。
    #[test]
    fn injection_warns_about_process_visibility() {
        let warnings = warnings_for(&client("claude-code", true, true), true);
        assert!(warnings.iter().any(|item| item.contains("其他进程可见")), "缺少环境变量可见性提醒");
        assert!(warnings.iter().any(|item| item.contains("不写入任何配置文件")));
    }

    /// 没有可执行文件时给明确错误，而不是 spawn 一个空路径。
    #[test]
    fn launch_without_target_fails_clearly() {
        let mut target = client("claude-desktop", false, false);
        target.launch_target = None;
        let error = launch(&target, &provider(ProtocolKind::Anthropic), "https://x", "s").expect_err("应当报错");
        assert!(matches!(error, AppError::InvalidInput(_)));
    }

    /// 未注入时的提醒要说清"仅启动"，不能让用户以为密钥已经生效。
    #[test]
    fn non_injecting_launch_says_so() {
        let desktop = warnings_for(&client("claude-desktop", false, false), false);
        assert!(desktop.iter().any(|item| item.contains("不注入任何 API 配置")));
        let cli = warnings_for(&client("codex-cli", false, true), false);
        assert!(cli.iter().any(|item| item.contains("仅启动进程")));
    }

    /// descriptor_for 只接受已注册的客户端 id。
    #[test]
    fn descriptor_lookup_rejects_unknown_clients() {
        assert!(descriptor_for("claude-desktop").is_ok());
        assert!(descriptor_for("claude-code").is_ok());
        assert!(matches!(descriptor_for("not-a-client"), Err(AppError::InvalidInput(_))));
    }
}

