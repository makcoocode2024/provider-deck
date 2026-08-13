import type { ClientDescriptor, ProtocolKind } from "../domain/types";

interface ClientDefinition extends Omit<ClientDescriptor, "installed" | "detectedPath" | "configPath"> {
  commands: string[];
  configCandidates: Record<string, string[]>;
}

const allProtocols: ProtocolKind[] = ["openai", "anthropic", "gemini", "azure-openai", "custom"];

export const clientCatalog: ClientDefinition[] = [
  {
    id: "codex-cli",
    name: "OpenAI Codex CLI",
    platforms: ["windows", "macos", "linux"],
    protocols: ["openai", "azure-openai"],
    commands: ["codex"],
    configCandidates: {
      windows: ["%USERPROFILE%\\.codex\\config.toml"],
      macos: ["~/.codex/config.toml"],
      linux: ["~/.codex/config.toml"],
    },
    support: "verified",
    autoConfig: true,
    requiresRestart: true,
    // 与 clients.rs 对齐：Codex 的 model_providers.<id>.env_key 会指定一个环境变量名，
    // 该变量的值作为 Bearer 发出（本机 0.147.0 实测）。所以启动器注入是有效的。
    envInjection: true,
    guidance: "写入 model_providers 配置；密钥优先保存在系统凭据库，必要时需用户确认导出到环境变量。",
  },
  {
    id: "claude-code",
    name: "Claude Code",
    platforms: ["windows", "macos", "linux"],
    protocols: ["anthropic"],
    commands: ["claude"],
    configCandidates: {
      windows: ["%USERPROFILE%\\.claude\\settings.json"],
      macos: ["~/.claude/settings.json"],
      linux: ["~/.claude/settings.json"],
    },
    support: "verified",
    autoConfig: true,
    requiresRestart: true,
    guidance: "合并 settings.json 的 env 字段，并保留未知字段。密钥写入明文配置前必须再次确认。",
  },
  {
    id: "gemini-cli",
    name: "Gemini CLI",
    platforms: ["windows", "macos", "linux"],
    protocols: ["gemini"],
    commands: ["gemini"],
    configCandidates: {
      windows: ["%USERPROFILE%\\.gemini\\settings.json"],
      macos: ["~/.gemini/settings.json"],
      linux: ["~/.gemini/settings.json"],
    },
    support: "experimental",
    autoConfig: false,
    requiresRestart: true,
    guidance: "官方已公告产品迁移安排，当前仅生成环境变量说明，不自动覆盖配置。",
  },
  {
    id: "opencode",
    name: "OpenCode",
    platforms: ["windows", "macos", "linux"],
    protocols: allProtocols,
    commands: ["opencode"],
    configCandidates: {
      windows: ["%APPDATA%\\opencode\\opencode.jsonc"],
      macos: ["~/.config/opencode/opencode.jsonc"],
      linux: ["~/.config/opencode/opencode.jsonc"],
    },
    support: "verified",
    autoConfig: true,
    requiresRestart: true,
    guidance: "使用官方 provider 配置并通过 {env:...} 或 {file:...} 引用密钥。",
  },
  // 两款桌面客户端固定 manual + autoConfig:false + envInjection:false，
  // 与 clients.rs 的 gui_descriptor 对齐。它们按账号登录，没有公开的 API 端点字段，
  // configCandidates 刻意为空——填了会让浏览器模式声称有个能改的配置文件。
  {
    id: "claude-desktop",
    name: "Claude Desktop",
    platforms: ["windows", "macos"],
    protocols: ["anthropic"],
    commands: [],
    configCandidates: {},
    support: "manual",
    autoConfig: false,
    requiresRestart: true,
    envInjection: false,
    guidance: "本程序不修改客户端登录态，请在客户端内手动配置 API 地址与密钥。仅检测安装状态并提供启动入口。该应用按账号登录，没有公开的 API 端点或密钥字段，凭据存放在其内部会话存储中，Provider Deck 不会读写这些数据。要接第三方中转 API，请改用 Claude Code CLI。",
  },
  {
    id: "chatgpt-desktop",
    name: "ChatGPT Desktop",
    platforms: ["windows", "macos"],
    protocols: ["openai"],
    commands: [],
    configCandidates: {},
    support: "manual",
    autoConfig: false,
    requiresRestart: true,
    envInjection: false,
    guidance: "本程序不修改客户端登录态，请在客户端内手动配置 API 地址与密钥。仅检测安装状态。该应用按账号登录，没有公开的 API 端点或密钥字段，凭据存放在其内部会话存储中，Provider Deck 不会读写这些数据。要接第三方中转 API，请改用 Codex CLI。",
  },
  ...["VS Code", "Cursor", "Windsurf", "Cline", "Roo Code", "Continue"].map((name) => ({
    id: name.toLowerCase().replace(/\s+/g, "-"),
    name,
    platforms: ["windows", "macos", "linux"],
    protocols: allProtocols,
    commands: [name.toLowerCase().replace(/\s+/g, "")],
    configCandidates: {},
    support: "manual" as const,
    autoConfig: false,
    requiresRestart: true,
    guidance: "仅检测安装状态并提供手动配置指引，不修改扩展内部数据库。",
  })),
];
