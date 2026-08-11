export type ProtocolKind =
  | "openai"
  | "anthropic"
  | "gemini"
  | "azure-openai"
  | "custom";

export type ConnectionState = "untested" | "testing" | "connected" | "failed";
export type SupportLevel = "verified" | "experimental" | "manual" | "unsupported";
export type ClaudeModelProfile = "sonnet" | "opus" | "haiku";
export type CodexCompatibility = "full" | "function-tools-only" | "chat-proxy" | "responses-unsupported" | "unknown" | "not-applicable";

export interface ClaudeModelMappings {
  sonnet?: string;
  opus?: string;
  haiku?: string;
}

export interface ModelInfo {
  id: string;
  displayName: string;
  provider?: string;
  protocol: ProtocolKind;
  source: "server" | "known-rule" | "manual";
  capabilities: string[];
  contextWindow?: number;
}

export interface Provider {
  id: string;
  name: string;
  baseUrl: string;
  protocol: ProtocolKind;
  enabled: boolean;
  isCurrent: boolean;
  defaultModel?: string;
  claudeModelProfile?: ClaudeModelProfile;
  claudeExtendedContext?: boolean;
  claudeModelMappings?: ClaudeModelMappings;
  codexCompatibility?: CodexCompatibility;
  codexProbeModel?: string;
  codexProbeDetail?: string;
  models: ModelInfo[];
  connectionState: ConnectionState;
  confidence?: number;
  lastCheckedAt?: string;
  appliedClients: string[];
  errorSummary?: string;
}

export interface ProviderDraft {
  id?: string;
  name: string;
  baseUrl: string;
  apiKey: string;
  protocolHint?: ProtocolKind;
  timeoutSeconds: number;
  azureApiVersion?: string;
  defaultModel?: string;
  claudeModelProfile?: ClaudeModelProfile;
  claudeExtendedContext?: boolean;
  claudeModelMappings?: ClaudeModelMappings;
}

export interface ProbeResult {
  normalizedBaseUrl: string;
  protocol: ProtocolKind;
  confidence: number;
  models: ModelInfo[];
  codexCompatibility?: CodexCompatibility;
  codexProbeModel?: string;
  codexProbeDetail?: string;
  checkedEndpoints: string[];
  userMessage: string;
  technicalDetail?: string;
}

export interface ProviderTestCheck {
  id: string;
  label: string;
  status: "passed" | "failed" | "skipped";
  detail: string;
  latencyMs?: number;
}

export interface ProviderTestReport {
  providerId: string;
  model?: string;
  totalLatencyMs: number;
  checks: ProviderTestCheck[];
  replyPreview?: string;
}

export interface ClientDescriptor {
  id: string;
  name: string;
  platforms: string[];
  protocols: ProtocolKind[];
  installed: boolean;
  detectedPath?: string;
  configPath?: string;
  support: SupportLevel;
  autoConfig: boolean;
  requiresRestart: boolean;
  guidance: string;
}

export interface ConfigChange {
  clientId: string;
  clientName: string;
  targetPath?: string;
  support: SupportLevel;
  canWrite: boolean;
  format: "toml" | "json" | "jsonc" | "dotenv" | "manual";
  beforePreview: string;
  afterPreview: string;
  warnings: string[];
  expectedHash?: string;
}

export interface ApplyResult {
  clientId: string;
  success: boolean;
  backupId?: string;
  message: string;
  restartRequired: boolean;
}

export interface BackupRecord {
  id: string;
  clientId: string;
  targetPath: string;
  createdAt: string;
  size: number;
}

export type ChatRestoreMode = "merge" | "replace";

export interface ChatBackupRecord {
  id: string;
  fileName: string;
  path: string;
  createdAt: string;
  size: number;
  conversationCount: number;
  version: number;
}

export interface ChatCacheSummary {
  conversationCount: number;
  currentSessionCount: number;
  historicalConversationCount: number;
  messageCount: number;
  cachePath: string;
  backupDirectory: string;
  cacheStatus: "available" | "missing" | "damaged";
  cacheMessage?: string;
}

export interface ChatRestoreResult {
  success: boolean;
  message: string;
  importedCount: number;
  totalCount: number;
  currentSessionCount: number;
  historicalConversationCount: number;
  rollbackSnapshotId?: string;
}

export type ReasoningLevel = "low" | "medium" | "high";

export interface AppSettings {
  timeoutSeconds: number;
  proxyUrl: string;
  allowSelfSignedCertificates: boolean;
  generateOnly: boolean;
  clearClipboardSeconds: number;
  locale: "zh-CN" | "en-US";
  localProxyPort?: number;
  autoReasoningMode: boolean;
  manualReasoningLevel: ReasoningLevel;
  effectiveReasoningLevel: ReasoningLevel;
  reasoningMatchMessage?: string;
}

export const defaultSettings: AppSettings = {
  timeoutSeconds: 10,
  proxyUrl: "",
  allowSelfSignedCertificates: false,
  generateOnly: true,
  clearClipboardSeconds: 30,
  locale: "zh-CN",
  localProxyPort: undefined,
  autoReasoningMode: false,
  manualReasoningLevel: "high",
  effectiveReasoningLevel: "high",
  reasoningMatchMessage: undefined,
};
