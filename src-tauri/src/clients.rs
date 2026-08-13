use std::{env, path::PathBuf};
use directories::UserDirs;
use crate::model::{ClientDescriptor, ProtocolKind, SupportLevel};

fn platform() -> &'static str {
    if cfg!(windows) { "windows" } else if cfg!(target_os = "macos") { "macos" } else { "linux" }
}

/// 自动写入的目标配置文件。
///
/// claude-desktop / chatgpt-desktop 刻意不在此列：它们没有任何公开的 API 端点字段
/// 可写。`config::preview` 和 `config::apply` 都把这里的返回值当写入目标，返回 Some
/// 会让 Provider Deck 声称能改一个其实改不了的文件。它们的真实配置位置写在 guidance。
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

/// 按绝对路径探测，用于不在 PATH 上的 GUI 客户端。
///
/// 必须和 `find_command` 分开，不能合成一个"先查 PATH 再查路径"的函数：
/// Claude Desktop 的可执行文件也叫 claude.exe。一旦它的安装目录进了 PATH，
/// 合并后的探测就会让 Claude Desktop 和 Claude Code CLI 互相冒充，
/// 而后者是 auto_config 客户端——认错会把配置写去错误的地方。
fn find_installed_path(candidates: &[PathBuf]) -> Option<PathBuf> {
    candidates.iter().find(|path| path.exists()).cloned()
}

fn dir_from_env(key: &str) -> Option<PathBuf> {
    env::var_os(key).map(PathBuf::from).filter(|path| !path.as_os_str().is_empty())
}

fn home_dir() -> Option<PathBuf> {
    UserDirs::new().map(|dirs| dirs.home_dir().to_path_buf())
}

/// Claude Desktop 的安装位置。
///
/// Windows 一项本机实测确认：Squirrel 布局，根目录的 claude.exe 是引导器，
/// 真正的版本在同级 `app-<version>\` 下。要指向根目录那个——它会自己选当前版本，
/// 直接指向 app 目录会在下次自动更新后失效。macOS 两项按惯例排列，未在本机核实。
fn claude_desktop_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(local) = dir_from_env("LOCALAPPDATA") {
        candidates.push(local.join("AnthropicClaude").join("claude.exe"));
    }
    if let Some(home) = home_dir() {
        candidates.push(home.join("Applications").join("Claude.app"));
    }
    candidates.push(PathBuf::from("/Applications/Claude.app"));
    candidates
}

/// ChatGPT Desktop 的安装位置。
///
/// 全部未经本机核实——开发机上没装 ChatGPT Desktop。装的是同一发行商的
/// `OpenAI.Codex` MSIX，那是另一个产品，不能拿来冒充 ChatGPT Desktop，
/// 所以这里不去匹配它。探测不到只会让 installed 为 false：该客户端是 manual 级别、
/// 没有任何写入路径，误判为未安装的代价仅是少一条提示。
fn chatgpt_desktop_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(local) = dir_from_env("LOCALAPPDATA") {
        candidates.push(local.join("Programs").join("ChatGPT").join("ChatGPT.exe"));
        candidates.push(local.join("Packages").join("OpenAI.ChatGPT-Desktop_2p2nqsd0c76g0"));
    }
    candidates.push(PathBuf::from("/Applications/ChatGPT.app"));
    candidates
}

#[allow(clippy::too_many_arguments)]
fn descriptor(id: &str, name: &str, commands: &[&str], protocols: Vec<ProtocolKind>, support: SupportLevel, auto_config: bool, env_injection: bool, guidance: &str) -> ClientDescriptor {
    let detected_path = find_command(commands);
    ClientDescriptor {
        id: id.into(), name: name.into(), platforms: vec![platform().into()], protocols,
        installed: detected_path.is_some(), launch_target: detected_path.clone(), detected_path,
        config_path: config_path(id).map(|p| p.to_string_lossy().into_owned()),
        support, auto_config, requires_restart: true, guidance: guidance.into(), env_injection,
    }
}

/// GUI 桌面客户端的描述符。
///
/// 固定 `manual` + `auto_config: false` + `env_injection: false`，不开参数：
/// 这两款应用都没有公开的 API 端点字段，凭据存在 Electron 会话存储里。
/// `docs/client-adapters.md` 要求"只有公开稳定配置格式和本机字段均得到核对后，
/// 适配器才能标为 verified"，并禁止改动客户端内部数据库——两条都指向 manual。
/// 把这三项写成常量而不是入参，是为了让"给桌面客户端开写入口"这件事无法顺手完成。
fn gui_descriptor(id: &str, name: &str, platforms: &[&str], candidates: Vec<PathBuf>, protocols: Vec<ProtocolKind>, guidance: &str) -> ClientDescriptor {
    let detected = find_installed_path(&candidates);
    // 只有探测到的是文件才给启动目标。MSIX 命中的是数据目录，spawn 一个目录必然失败；
    // MSIX 要走 shell:AppsFolder，那条路径没核实过，所以留空而不是猜一个。
    let launch_target = detected.as_ref().filter(|path| path.is_file()).map(|path| path.to_string_lossy().into_owned());
    ClientDescriptor {
        id: id.into(), name: name.into(), platforms: platforms.iter().map(|item| (*item).to_string()).collect(), protocols,
        installed: detected.is_some(), launch_target,
        detected_path: detected.map(|path| path.to_string_lossy().into_owned()),
        config_path: None,
        support: SupportLevel::Manual, auto_config: false, requires_restart: true, guidance: guidance.into(), env_injection: false,
    }
}

pub fn detect_all() -> Vec<ClientDescriptor> {
    let all = vec![ProtocolKind::Openai, ProtocolKind::Anthropic, ProtocolKind::Gemini, ProtocolKind::AzureOpenai, ProtocolKind::Custom];
    vec![
        // codex-cli 的 env_injection 改成 true，依据是对本机 codex 0.147.0 可执行文件的
        // 实测（不是文档推断）：`model_providers.<id>.env_key` 指定一个环境变量名，
        // 该变量的值会作为 Authorization: Bearer 发出。三点实测结论：
        //   1. env_key 指向未设置的变量时，即使同时写了 experimental_bearer_token，
        //      也直接报 "Missing environment variable" 并退出——env_key 是硬要求，
        //      不是"取不到就回落到 bearer token"。所以配置里两者必须一致地成对出现。
        //   2. 不写 env_key 时，注入 OPENAI_API_KEY 对自定义 provider 完全无效
        //      （实测发出的仍是 auth.json 里的账号凭据）。这就是模块二"注入
        //      OPENAI_API_KEY"必须配一个 env_key 才成立的原因。
        //   3. env_key = "OPENAI_API_KEY" 且变量已注入时，发出的正是注入值。
        descriptor("codex-cli", "OpenAI Codex CLI", &["codex"], vec![ProtocolKind::Openai, ProtocolKind::AzureOpenai], SupportLevel::Verified, true, true, "使用公开 TOML model_providers 结构。"),
        // claude-code 的 env_injection 是 true：config::merge_claude 已经在写
        // ANTHROPIC_BASE_URL / ANTHROPIC_API_KEY 这两个环境变量，只是写进了
        // settings.json 的 env 字段。既然这两个名字确认有人读，启动器就能改成
        // 运行时注入，把明文密钥从配置文件里彻底拿掉。
        descriptor("claude-code", "Claude Code", &["claude"], vec![ProtocolKind::Anthropic], SupportLevel::Verified, true, true, "合并 ~/.claude/settings.json 的 env 字段。"),
        descriptor("gemini-cli", "Gemini CLI", &["gemini"], vec![ProtocolKind::Gemini], SupportLevel::Experimental, false, false, "官方已公告迁移安排，仅生成手动配置说明。"),
        descriptor("opencode", "OpenCode", &["opencode"], all.clone(), SupportLevel::Verified, true, false, "使用官方 JSON Provider 结构。"),
        gui_descriptor("claude-desktop", "Claude Desktop", &["windows", "macos"], claude_desktop_candidates(), vec![ProtocolKind::Anthropic],
            "本程序不修改客户端登录态，请在客户端内手动配置 API 地址与密钥。仅检测安装状态并提供启动入口。该应用按账号登录，没有公开的 API 端点或密钥字段，凭据存放在其内部会话存储中，Provider Deck 不会读写这些数据。要接第三方中转 API，请改用 Claude Code CLI。"),
        gui_descriptor("chatgpt-desktop", "ChatGPT Desktop", &["windows", "macos"], chatgpt_desktop_candidates(), vec![ProtocolKind::Openai],
            "本程序不修改客户端登录态，请在客户端内手动配置 API 地址与密钥。仅检测安装状态。该应用按账号登录，没有公开的 API 端点或密钥字段，凭据存放在其内部会话存储中，Provider Deck 不会读写这些数据。要接第三方中转 API，请改用 Codex CLI。"),
        descriptor("vs-code", "VS Code", &["code"], all.clone(), SupportLevel::Manual, false, false, "仅检测安装状态，不修改扩展内部数据。"),
        descriptor("cursor", "Cursor", &["cursor"], all.clone(), SupportLevel::Manual, false, false, "仅检测安装状态，不修改内部数据库。"),
        descriptor("windsurf", "Windsurf", &["windsurf"], all.clone(), SupportLevel::Manual, false, false, "仅检测安装状态，不修改内部数据库。"),
        descriptor("cline", "Cline", &[], all.clone(), SupportLevel::Manual, false, false, "请在扩展设置中手动配置。"),
        descriptor("roo-code", "Roo Code", &[], all.clone(), SupportLevel::Manual, false, false, "请在扩展设置中手动配置。"),
        descriptor("continue", "Continue", &[], all, SupportLevel::Manual, false, false, "请按 Continue 当前公开文档手动配置。"),
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

#[cfg(test)]
mod tests {
    use super::*;

    fn find(id: &str) -> ClientDescriptor {
        detect_all().into_iter().find(|item| item.id == id).expect("客户端未注册")
    }

    /// 两款桌面客户端都没有可写的 API 端点字段。config::preview 用 config_path
    /// 决定写入目标、用 auto_config 决定 can_write，任一处松口都会让 Provider Deck
    /// 声称能改一个其实改不了的文件。
    #[test]
    fn desktop_clients_expose_no_write_surface() {
        for id in ["claude-desktop", "chatgpt-desktop"] {
            let client = find(id);
            assert_eq!(client.support, SupportLevel::Manual, "{id} 必须是 manual 级别");
            assert!(!client.auto_config, "{id} 不得开启自动写入");
            assert!(!client.env_injection, "{id} 没有已核实的环境变量入口");
            assert!(config_path(id).is_none(), "{id} 不得有自动写入目标路径");
            assert!(client.config_path.is_none(), "{id} 不得对前端暴露配置路径");
        }
    }

    /// 桌面客户端的 guidance 必须说明"不读写其内部数据"。这是 README 与
    /// docs/client-adapters.md 的承诺，用户在客户端页面看到的就是这段文字。
    #[test]
    fn desktop_guidance_states_internal_data_is_untouched() {
        for id in ["claude-desktop", "chatgpt-desktop"] {
            assert!(find(id).guidance.contains("不会读写"), "{id} 的引导文案缺少内部数据声明");
        }
    }

    /// 现有四个客户端的支持级别不能被本次改动带偏。
    #[test]
    fn existing_clients_keep_their_support_levels() {
        for (id, support, auto_config) in [
            ("codex-cli", SupportLevel::Verified, true),
            ("claude-code", SupportLevel::Verified, true),
            ("gemini-cli", SupportLevel::Experimental, false),
            ("opencode", SupportLevel::Verified, true),
        ] {
            let client = find(id);
            assert_eq!(client.support, support, "{id} 支持级别被改动");
            assert_eq!(client.auto_config, auto_config, "{id} 自动写入开关被改动");
            assert!(config_path(id).is_some(), "{id} 丢了配置路径");
        }
    }

    /// claude-desktop 与 claude-code 的可执行文件同名（claude.exe）。
    /// 探测必须分开：认错会把 Anthropic 协议的配置写去桌面版，而它根本不读配置文件。
    #[test]
    fn claude_desktop_and_claude_code_stay_distinct() {
        let desktop = find("claude-desktop");
        let cli = find("claude-code");
        assert!(desktop.config_path.is_none());
        assert_eq!(cli.config_path, config_path("claude-code").map(|p| p.to_string_lossy().into_owned()));
        assert!(!desktop.auto_config && cli.auto_config);
    }

    /// 只有确认会读环境变量的客户端才允许注入。密钥进子进程环境后，
    /// 同一用户的任何进程都能读到，没人读的注入是白担风险。
    ///
    /// codex-cli 从"不注入"改成"注入"的依据是本机 codex 0.147.0 实测，
    /// 见 detect_all 里那段注释。opencode / gemini-cli 仍然没实测过，保持 false。
    #[test]
    fn only_verified_env_readers_allow_injection() {
        assert!(find("claude-code").env_injection, "claude-code 已确认读 ANTHROPIC_API_KEY");
        assert!(find("codex-cli").env_injection, "codex-cli 已实测 env_key 指定的变量会作为 Bearer 发出");
        for id in ["opencode", "gemini-cli", "claude-desktop", "chatgpt-desktop", "vs-code"] {
            assert!(!find(id).env_injection, "{id} 未核实环境变量入口，不得注入");
        }
    }

    /// 桌面客户端的引导文案必须含"不修改客户端登录态"这句原话。
    /// 这是本次任务的边界红线，界面上用户看到的就是这段文字。
    #[test]
    fn desktop_guidance_states_login_state_is_untouched() {
        for id in ["claude-desktop", "chatgpt-desktop"] {
            let guidance = find(id).guidance;
            assert!(guidance.contains("本程序不修改客户端登录态"), "{id} 缺少登录态声明");
            assert!(guidance.contains("手动配置 API 地址与密钥"), "{id} 缺少手动配置引导");
        }
    }

    /// 桌面客户端的探测路径来自固定候选表，未安装时 installed 为 false、
    /// launch_target 为 None——不能因为探测不到就抛错或给一个猜的路径。
    #[test]
    fn desktop_detection_degrades_when_not_installed() {
        for id in ["claude-desktop", "chatgpt-desktop"] {
            let client = find(id);
            // 开发机上装没装都不确定，所以断言的是两种状态各自的自洽性。
            if client.installed {
                assert!(client.detected_path.is_some(), "{id} 标为已安装却没有探测路径");
            } else {
                assert!(client.detected_path.is_none(), "{id} 标为未安装却带着探测路径");
                assert!(client.launch_target.is_none(), "{id} 未安装时不得给启动目标");
            }
        }
    }

    /// 候选路径不能是相对路径：spawn 一个相对路径会按子进程的工作目录解析，
    /// 拉起哪个文件就不确定了。
    ///
    /// 判据不用 `Path::is_absolute()`：它按**当前编译目标**的规则判断，
    /// 在 Windows 上 `/Applications/Claude.app` 会被判成非绝对（缺盘符），
    /// 而那条路径在它真正生效的 macOS 上是绝对的。所以这里查的是
    /// "以分隔符或盘符开头"，对两套平台的候选都成立。
    #[test]
    fn desktop_candidates_are_never_relative() {
        for candidate in claude_desktop_candidates().into_iter().chain(chatgpt_desktop_candidates()) {
            let text = candidate.to_string_lossy().into_owned();
            let rooted = text.starts_with('/') || text.starts_with('\\') || text.chars().nth(1) == Some(':');
            assert!(rooted, "候选路径是相对路径：{text}");
        }
    }

    /// 旧数据少字段时按最保守的方向降级：support 退到 manual、两个开关退到 false。
    /// 反过来默认成 verified/true 会让一条来路不明的记录拿到写入和注入资格。
    #[test]
    fn missing_fields_degrade_to_the_safest_defaults() {
        let client: ClientDescriptor = serde_json::from_str(r#"{"id":"legacy","name":"旧客户端"}"#)
            .expect("少字段的旧数据必须能反序列化");
        assert_eq!(client.support, SupportLevel::Manual);
        assert!(!client.auto_config, "缺省不得开启自动写入");
        assert!(!client.env_injection, "缺省不得开启环境变量注入");
        assert!(client.launch_target.is_none());
        assert!(!client.installed);
    }

    /// support 的序列化取值必须是小写字符串：前端 SupportLevel 联合类型
    /// 和 CSS 类名都按这四个字面量匹配。
    #[test]
    fn support_level_serializes_to_lowercase_ids() {
        for (level, id) in [
            (SupportLevel::Verified, "verified"),
            (SupportLevel::Experimental, "experimental"),
            (SupportLevel::Manual, "manual"),
            (SupportLevel::Unsupported, "unsupported"),
        ] {
            assert_eq!(serde_json::to_string(&level).unwrap(), format!("\"{id}\""));
            assert_eq!(level.as_str(), id);
        }
    }

    /// launch_target 只在探测到可执行文件时给值。MSIX 命中的是数据目录，
    /// 拿目录去 spawn 必然失败——宁可没有启动按钮，不要给一个必然报错的。
    #[test]
    fn launch_target_is_never_a_directory() {
        for client in detect_all() {
            if let Some(target) = &client.launch_target {
                assert!(!PathBuf::from(target).is_dir(), "{} 的启动目标是目录", client.id);
            }
        }
    }

    /// 客户端 id 不能重复：detect_all 的结果在 config::preview 里按 id 查找，
    /// 重复会让查到的那一条取决于顺序。
    #[test]
    fn client_ids_are_unique() {
        let mut ids: Vec<String> = detect_all().into_iter().map(|item| item.id).collect();
        let total = ids.len();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), total, "存在重复的客户端 id");
    }
}

