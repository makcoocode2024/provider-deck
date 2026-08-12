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

// —— 推理能力描述符。以下类型是后端 reasoning_capability.rs / reasoning_selection.rs
// 的镜像，字段名沿用 serde 的 camelCase 契约。前端**只读**这些结构，不派生档位。

/** 三态结论。unknown 必须可表达：未探明 ≠ 不支持。 */
export type ReasoningSupport = "unknown" | "unsupported" | "supported";

/**
 * 置信度阶梯，与四级证据阶梯一一对应。后端 `ReasoningConfidence` 的镜像。
 *
 * `verified` 保留给未来的 capability validation，**不由 runtime verification 产生**。
 * 后端生产代码目前没有任何一处写入这一档（唯一出现处是 `lib.rs` 的测试 fixture），
 * Phase D 也不写、不读、不展示它。运行时验证的结论一律走 {@link RuntimeVerification}，
 * 落在 {@link Provider.reasoningVerifications}，不回写 {@link ModelInfo.reasoning}。
 */
export type ReasoningConfidence = "unknown" | "declared" | "validated" | "verified";

export type EvidenceSource =
  | "model-list-metadata"
  | "introspection"
  | "validation-probe"
  | "capability-validation"
  | "billed-probe"
  | "runtime-observation"
  | "manual-override";

/** 语义档位。跨协议可比较，是选择的权威字段。 */
export type ReasoningTier = "off" | "light" | "standard" | "deep" | "max";

export interface ReasoningKey {
  baseUrl: string;
  modelId: string;
}

export interface ReasoningEvidence {
  source: EvidenceSource;
  endpoint?: string;
  /** 已脱敏的证据摘要，可直接展示。 */
  detail: string;
  observedAt: string;
}

/** 控制形态。内部标签联合，判别字段是 `kind`。 */
export type ReasoningControl =
  | { kind: "effortEnum"; values: string[] }
  | { kind: "tokenBudget"; min: number; max: number; offAllowed: boolean; dynamicSentinel?: number | null }
  | { kind: "booleanToggle" }
  | { kind: "none" };

/** 档位到线上参数的绑定。内部标签联合，判别字段是 `kind`。 */
export type ReasoningBinding =
  | { kind: "effort"; value: string }
  | { kind: "budget"; tokens: number }
  | { kind: "dynamicBudget"; sentinel: number }
  | { kind: "enabled" }
  | { kind: "disabled" }
  | { kind: "omitted" };

export interface ReasoningTierOption {
  tier: ReasoningTier;
  /** 稳定标识，UI 用它作为选项 value。 */
  id: string;
  label: string;
  binding: ReasoningBinding;
  /** 展示给用户的实际线上取值，让映射关系可见。 */
  wireSummary: string;
}

export interface ReasoningConstraints {
  budgetBelowMaxTokens?: boolean;
  locksSamplingParams?: boolean;
  /** 该模型无法关闭推理。 */
  cannotDisable?: boolean;
  notes?: string[];
}

export interface ReasoningCapability {
  key: ReasoningKey;
  support: ReasoningSupport;
  control: ReasoningControl;
  tiers: ReasoningTierOption[];
  defaultTier?: ReasoningTier | null;
  defaultReason?: string | null;
  constraints: ReasoningConstraints;
  confidence: ReasoningConfidence;
  evidence: ReasoningEvidence[];
  discoveredAt: string;
  ttlSeconds: number;
}

export type SelectionSource = "user" | "legacyFallback" | "capabilityDefault";

/**
 * 用户的推理档位**选择**，归属 `(provider, model_id)`，换端点仍然有效。
 * 与 `ReasoningCapability`（发现到的事实，归属 `(base_url, model_id)`）生命周期不同。
 */
export interface ReasoningSelection {
  modelId: string;
  tier?: ReasoningTier | null;
  explicitBinding?: ReasoningBinding | null;
  source: SelectionSource;
  chosenAt: string;
}

// —— 运行时验证。后端 reasoning_verification.rs 的镜像。
//
// 与上面的能力发现是两条独立链路：能力发现回答"这个端点的这个模型支持什么"，写
// `ModelInfo.reasoning`；运行时验证回答"用户在这个端点上试过什么、结果如何"，写
// `Provider.reasoningVerifications`。两者互不回写——一次成功的用户请求不构成探测事实。

/** 三态判别值。与后端 `VerificationResult` 的 serde tag 同名。 */
export type VerificationStatus = "confirmed" | "rejected" | "failed";

/**
 * 验证结论。内部标签联合，判别字段是 `status`（后端 `#[serde(tag = "status")]`）。
 *
 * `rejected` 与 `failed` 都**不等于**"不支持推理"：前者是"这次响应里没看到推理产物"，
 * 后者是"这次请求没走通"。能力结论只由 {@link ReasoningCapability.support} 表达。
 */
export type VerificationResult =
  | { status: "confirmed" }
  | { status: "rejected"; reason: string }
  | { status: "failed"; error: string };

/**
 * 单次运行时验证记录。只含判定所需的字段——不含 API key、请求体、响应原文。
 *
 * 归属 `(baseUrl, modelId)`：换端点后旧记录一律作废（后端 `retain_for_endpoint` 负责剪枝），
 * 因为它断言的是某个端点的运行时行为，不是用户意图。
 */
export interface RuntimeVerification {
  modelId: string;
  /** 已归一化的 base URL。 */
  baseUrl: string;
  tier: ReasoningTier;
  /** 验证时实际发出的绑定，由后端从能力表派生，供 UI 显示"当时发了什么"。 */
  binding: ReasoningBinding;
  result: VerificationResult;
  /** RFC3339 时间戳。 */
  verifiedAt: string;
  protocol: string;
}

export interface ModelInfo {
  id: string;
  displayName: string;
  provider?: string;
  protocol: ProtocolKind;
  source: "server" | "known-rule" | "manual";
  capabilities: string[];
  contextWindow?: number;
  parameterCountBillions?: number;
  reasoning?: ReasoningCapability;
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
  reasoningSelections?: ReasoningSelection[];
  /**
   * 用户主动发起的运行时验证历史，key 为 modelId，值按时间追加。
   *
   * 可选是因为旧的本地数据没有这个字段；后端始终会序列化它（`#[serde(default)]`）。
   */
  reasoningVerifications?: Record<string, RuntimeVerification[]>;
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
  reasoningSelections?: ReasoningSelection[];
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
  reasoningNote?: string;
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

/**
 * 旧的三档全局档位。**不是**推理能力的来源，只是"能力未探明时的回退档位"。
 * 真实能力一律走 {@link ReasoningCapability}；新代码不要基于这个类型派生推理档位 UI。
 */
export type ReasoningLevel = "low" | "medium" | "high";

/**
 * 上面那个旧枚举的 serde 契约镜像，供设置页的**回退档位**下拉使用。
 *
 * 这不是推理档位清单：推理档位一律来自 `ReasoningCapability.tiers`，会随服务端声明增减。
 * 这三个成员是后端 `model::ReasoningLevel` 的全部变体，是一个封闭枚举的取值集合，
 * 既不会因为某个网关多声明一个 `ultra` 而变化，也不参与任何能力判断。
 * 下拉直接显示这里的原始取值，不做中文翻译——后端没有为这个旧枚举提供展示 label，
 * 前端就不发明一个。
 */
export const legacyReasoningLevels: readonly ReasoningLevel[] = ["low", "medium", "high"];

export interface AppSettings {
  timeoutSeconds: number;
  proxyUrl: string;
  allowSelfSignedCertificates: boolean;
  generateOnly: boolean;
  clearClipboardSeconds: number;
  locale: "zh-CN" | "en-US";
  localProxyPort?: number;
  manualReasoningLevel: ReasoningLevel;
  /** 由 `manualReasoningLevel` 派生，供 legacy fallback 使用。前端不自行计算。 */
  effectiveReasoningLevel: ReasoningLevel;
}

export const defaultSettings: AppSettings = {
  timeoutSeconds: 10,
  proxyUrl: "",
  allowSelfSignedCertificates: false,
  generateOnly: true,
  clearClipboardSeconds: 30,
  locale: "zh-CN",
  localProxyPort: undefined,
  manualReasoningLevel: "high",
  effectiveReasoningLevel: "high",
};
