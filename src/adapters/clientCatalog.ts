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
