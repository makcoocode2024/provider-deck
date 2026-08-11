import { invoke } from "@tauri-apps/api/core";
import { clientCatalog } from "../adapters/clientCatalog";
import type {
  AppSettings,
  ApplyResult,
  BackupRecord,
  ChatBackupRecord,
  ChatCacheSummary,
  ChatRestoreMode,
  ChatRestoreResult,
  ClientDescriptor,
  ConfigChange,
  ProbeResult,
  Provider,
  ProviderDraft,
  ProviderTestReport,
} from "../domain/types";
import { defaultSettings } from "../domain/types";
import { normalizeBaseUrl } from "../domain/url";

export interface AppBackend {
  listProviders(): Promise<Provider[]>;
  getProviderApiKey(providerId: string): Promise<string>;
  saveProvider(draft: ProviderDraft, probe: ProbeResult): Promise<Provider>;
  deleteProvider(id: string): Promise<void>;
  setCurrentProvider(id: string): Promise<Provider[]>;
  probeProvider(draft: ProviderDraft): Promise<ProbeResult>;
  reprobeProvider(id: string): Promise<Provider>;
  refreshProviderModels(id: string): Promise<Provider>;
  testProvider(providerId: string, modelId?: string): Promise<ProviderTestReport>;
  detectClients(): Promise<ClientDescriptor[]>;
  previewChanges(providerId: string, clientIds: string[]): Promise<ConfigChange[]>;
  applyChanges(providerId: string, changes: ConfigChange[]): Promise<ApplyResult[]>;
  listBackups(): Promise<BackupRecord[]>;
  restoreBackup(id: string): Promise<void>;
  listChatBackups(): Promise<ChatBackupRecord[]>;
  chatCacheSummary(): Promise<ChatCacheSummary>;
  exportChatBackup(): Promise<ChatBackupRecord>;
  restoreChatBackupPayload(payload: string, mode: ChatRestoreMode): Promise<ChatRestoreResult>;
  restoreChatCache(mode: ChatRestoreMode): Promise<ChatRestoreResult>;
  rollbackChatRestore(snapshotId: string): Promise<ChatRestoreResult>;
  getSettings(): Promise<AppSettings>;
  saveSettings(settings: AppSettings): Promise<AppSettings>;
  exportProviders(): Promise<string>;
  importProviders(payload: string): Promise<Provider[]>;
  diagnostics(): Promise<Record<string, string>>;
}

const isTauri = () => typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

class TauriBackend implements AppBackend {
  listProviders = () => invoke<Provider[]>("list_providers");
  getProviderApiKey = (providerId: string) => invoke<string>("get_provider_api_key", { providerId });
  saveProvider = (draft: ProviderDraft, probe: ProbeResult) =>
    invoke<Provider>("save_provider", { draft, probe });
  deleteProvider = (id: string) => invoke<void>("delete_provider", { id });
  setCurrentProvider = (id: string) => invoke<Provider[]>("set_current_provider", { id });
  probeProvider = (draft: ProviderDraft) => invoke<ProbeResult>("probe_provider", { draft });
  reprobeProvider = (id: string) => invoke<Provider>("reprobe_provider", { id });
  refreshProviderModels = (id: string) => invoke<Provider>("refresh_provider_models", { providerId: id });
  testProvider = (providerId: string, modelId?: string) => invoke<ProviderTestReport>("test_provider", { providerId, modelId });
  detectClients = () => invoke<ClientDescriptor[]>("detect_clients");
  previewChanges = (providerId: string, clientIds: string[]) =>
    invoke<ConfigChange[]>("preview_changes", { providerId, clientIds });
  applyChanges = (providerId: string, changes: ConfigChange[]) =>
    invoke<ApplyResult[]>("apply_changes", { providerId, changes });
  listBackups = () => invoke<BackupRecord[]>("list_backups");
  restoreBackup = (id: string) => invoke<void>("restore_backup", { id });
  listChatBackups = () => invoke<ChatBackupRecord[]>("list_chat_backups");
  chatCacheSummary = () => invoke<ChatCacheSummary>("chat_cache_summary");
  exportChatBackup = () => invoke<ChatBackupRecord>("export_chat_backup");
  restoreChatBackupPayload = (payload: string, mode: ChatRestoreMode) => invoke<ChatRestoreResult>("restore_chat_backup_payload", { payload, mode });
  restoreChatCache = (mode: ChatRestoreMode) => invoke<ChatRestoreResult>("restore_chat_cache", { mode });
  rollbackChatRestore = (snapshotId: string) => invoke<ChatRestoreResult>("rollback_chat_restore", { snapshotId });
  getSettings = () => invoke<AppSettings>("get_settings");
  saveSettings = (settings: AppSettings) => invoke<AppSettings>("save_settings", { settings });
  exportProviders = () => invoke<string>("export_providers");
  importProviders = (payload: string) => invoke<Provider[]>("import_providers", { payload });
  diagnostics = () => invoke<Record<string, string>>("diagnostics");
}

class BrowserBackend implements AppBackend {
  private providerKey = "provider-deck.e2e.providers";
  private settingsKey = "provider-deck.e2e.settings";
  private providerSecrets = new Map<string, string>();
  private secretKey(id: string) { return `provider-deck.e2e.secret.${id}`; }

  private ensureTestMode() {
    if (import.meta.env.MODE !== "test" && import.meta.env.VITE_E2E !== "1") {
      throw new Error("当前是浏览器预览模式。网络探测和本地配置写入需要 Tauri 桌面后端。");
    }
  }

  async listProviders() {
    return JSON.parse(localStorage.getItem(this.providerKey) ?? "[]") as Provider[];
  }

  async getProviderApiKey(providerId: string) {
    this.ensureTestMode();
    const delay = Number(localStorage.getItem("provider-deck.e2e.secret-delay-ms") ?? "0");
    if (Number.isFinite(delay) && delay > 0) await new Promise((resolve) => setTimeout(resolve, delay));
    const secret = this.providerSecrets.get(providerId) ?? localStorage.getItem(this.secretKey(providerId));
    if (!secret) throw new Error("无法读取已保存的 API Key");
    return secret;
  }

  async saveProvider(draft: ProviderDraft, probe: ProbeResult) {
    if (localStorage.getItem("provider-deck.e2e.fail-save") === "1") {
      throw new Error("测试后端模拟保存失败");
    }
    const providers = await this.listProviders();
    const apiKey = draft.apiKey || (draft.id ? await this.getProviderApiKey(draft.id) : "");
    if (!apiKey) throw new Error("API Key 不能为空");
    const provider: Provider = {
      id: draft.id ?? crypto.randomUUID(),
      name: draft.name,
      baseUrl: probe.normalizedBaseUrl,
      protocol: probe.protocol,
      enabled: true,
      isCurrent: providers.length === 0,
      defaultModel: draft.defaultModel ?? probe.models[0]?.id,
      claudeModelProfile: draft.claudeModelProfile,
      claudeExtendedContext: draft.claudeExtendedContext,
      claudeModelMappings: draft.claudeModelMappings,
      codexCompatibility: probe.codexCompatibility,
      codexProbeModel: probe.codexProbeModel,
      codexProbeDetail: probe.codexProbeDetail,
      models: probe.models,
      connectionState: "connected",
      confidence: probe.confidence,
      lastCheckedAt: new Date().toISOString(),
      appliedClients: [],
    };
    const next = [...providers.filter((item) => item.id !== provider.id), provider];
    localStorage.setItem(this.providerKey, JSON.stringify(next));
    this.providerSecrets.set(provider.id, apiKey);
    localStorage.setItem(this.secretKey(provider.id), apiKey);
    return provider;
  }

  async deleteProvider(id: string) {
    localStorage.setItem(this.providerKey, JSON.stringify((await this.listProviders()).filter((p) => p.id !== id)));
    this.providerSecrets.delete(id);
    localStorage.removeItem(this.secretKey(id));
  }

  async setCurrentProvider(id: string) {
    const providers = (await this.listProviders()).map((p) => ({ ...p, isCurrent: p.id === id }));
    localStorage.setItem(this.providerKey, JSON.stringify(providers));
    return providers;
  }

  async probeProvider(draft: ProviderDraft): Promise<ProbeResult> {
    this.ensureTestMode();
    const apiKey = draft.apiKey || (draft.id ? await this.getProviderApiKey(draft.id) : "");
    if (!apiKey) throw new Error("API Key 不能为空");
    const normalized = normalizeBaseUrl(draft.baseUrl).value;
    const protocol = draft.protocolHint && draft.protocolHint !== "custom" ? draft.protocolHint : "openai";
    const models = protocol === "anthropic" ? [
      { id: "agnes-2.0-flash", displayName: "Agnes 2.0 Flash", protocol, source: "server" as const, capabilities: [], contextWindow: 200_000 },
      { id: "agnes-2.0-pro", displayName: "Agnes 2.0 Pro", protocol, source: "server" as const, capabilities: [], contextWindow: 200_000 },
      { id: "agnes-2.0-lite", displayName: "Agnes 2.0 Lite", protocol, source: "server" as const, capabilities: [], contextWindow: 200_000 },
    ] : [
      { id: "test-coder", displayName: "test-coder", protocol, source: "server" as const, capabilities: [] },
    ];
    return {
      normalizedBaseUrl: normalized,
      protocol,
      confidence: 0.94,
      models,
      codexCompatibility: protocol === "openai" ? "chat-proxy" : "not-applicable",
      codexProbeModel: protocol === "openai" ? models[0]?.id : undefined,
      codexProbeDetail: protocol === "openai" ? "测试后端模拟：Responses 不可用，Chat Completions function 工具可用。" : undefined,
      checkedEndpoints: [`${normalized}/models`],
      userMessage: protocol === "openai" ? "测试后端已启用 Codex 本地兼容桥。" : "测试后端已完成模型列表探测。",
    };
  }

  async reprobeProvider(id: string): Promise<Provider> {
    this.ensureTestMode();
    const providers = await this.listProviders();
    const provider = providers.find((item) => item.id === id);
    if (!provider) throw new Error(`未找到服务：${id}`);
    const apiKey = await this.getProviderApiKey(id);
    if (!apiKey) throw new Error("无法读取已保存的 API Key");
    const probe = await this.probeProvider({
      id: provider.id, name: provider.name, baseUrl: provider.baseUrl, apiKey,
      protocolHint: provider.protocol, timeoutSeconds: 10,
    });
    const refreshed: Provider = {
      ...provider, baseUrl: probe.normalizedBaseUrl, protocol: probe.protocol, models: probe.models,
      codexCompatibility: probe.codexCompatibility, codexProbeModel: probe.codexProbeModel, codexProbeDetail: probe.codexProbeDetail,
      defaultModel: provider.defaultModel && (probe.models.length === 0 || probe.models.some((model) => model.id === provider.defaultModel)) ? provider.defaultModel : probe.models[0]?.id,
      connectionState: "connected", confidence: probe.confidence, lastCheckedAt: new Date().toISOString(),
      errorSummary: undefined,
    };
    localStorage.setItem(this.providerKey, JSON.stringify(providers.map((item) => item.id === id ? refreshed : item)));
    return refreshed;
  }

  async refreshProviderModels(id: string): Promise<Provider> {
    const provider = await this.reprobeProvider(id);
    return provider;
  }

  async testProvider(providerId: string, modelId?: string): Promise<ProviderTestReport> {
    this.ensureTestMode();
    const provider = (await this.listProviders()).find((item) => item.id === providerId);
    if (!provider) throw new Error(`未找到服务：${providerId}`);
    const failed = localStorage.getItem("provider-deck.e2e.fail-provider-test") === "1";
    const model = modelId || provider.defaultModel || provider.models[0]?.id;
    return {
      providerId,
      model,
      totalLatencyMs: 42,
      checks: [
        { id: "connectivity", label: "连通性与身份验证", status: "passed", detail: "模型接口可访问，身份验证通过。", latencyMs: 18 },
        failed
          ? { id: "conversation", label: "最小真实对话", status: "failed", detail: "测试后端模拟：模型不可用或无访问权限。", latencyMs: 24 }
          : { id: "conversation", label: "最小真实对话", status: "passed", detail: "模型已成功生成回复。", latencyMs: 24 },
      ],
      replyPreview: failed ? undefined : "OK",
    };
  }

  async detectClients() {
    return clientCatalog.map((definition, index) => ({
      id: definition.id,
      name: definition.name,
      platforms: definition.platforms,
      protocols: definition.protocols,
      support: definition.support,
      autoConfig: definition.autoConfig,
      requiresRestart: definition.requiresRestart,
      guidance: definition.guidance,
      installed: index < 2,
      detectedPath: index < 2 ? `C:\\Tools\\${definition.id}.exe` : undefined,
    }));
  }

  async previewChanges(_providerId: string, clientIds: string[]) {
    this.ensureTestMode();
    return clientIds.map((clientId) => {
      const client = clientCatalog.find((item) => item.id === clientId)!;
      return {
        clientId,
        clientName: client.name,
        support: client.support,
        canWrite: client.autoConfig,
        format: client.autoConfig ? (clientId === "codex-cli" ? "toml" : "json") : "manual",
        beforePreview: "# 保留现有配置",
        afterPreview: "+ 已添加 Provider Deck 配置（密钥已隐藏）",
        warnings: client.autoConfig ? [] : [client.guidance],
      } as ConfigChange;
    });
  }

  async applyChanges(_providerId: string, changes: ConfigChange[]) {
    this.ensureTestMode();
    return changes.map((change) => ({
      clientId: change.clientId,
      success: change.canWrite,
      message: change.canWrite ? "测试模式写入完成" : "请按手动指引配置",
      restartRequired: true,
    }));
  }

  async listBackups() { return []; }
  async restoreBackup(id: string) { void id; this.ensureTestMode(); }
  async listChatBackups() { return [] as ChatBackupRecord[]; }
  async chatCacheSummary(): Promise<ChatCacheSummary> {
    return {
      conversationCount: 0,
      currentSessionCount: 0,
      historicalConversationCount: 0,
      messageCount: 0,
      cachePath: "浏览器测试模式不读取本机缓存",
      backupDirectory: "浏览器测试模式不写入备份目录",
      cacheStatus: "missing",
    };
  }
  async exportChatBackup(): Promise<ChatBackupRecord> {
    this.ensureTestMode();
    return {
      id: crypto.randomUUID(), fileName: "provider-deck-codex-chats-test.pdbchat.json", path: "浏览器测试模式",
      createdAt: new Date().toISOString(), size: 0, conversationCount: 0, version: 3,
    };
  }
  async restoreChatBackupPayload(payload: string, mode: ChatRestoreMode): Promise<ChatRestoreResult> {
    void payload;
    void mode;
    this.ensureTestMode();
    return { success: false, message: "浏览器测试模式不读取本机加密聊天备份。请使用 Tauri 桌面版。", importedCount: 0, totalCount: 0, currentSessionCount: 0, historicalConversationCount: 0 };
  }
  async restoreChatCache(mode: ChatRestoreMode): Promise<ChatRestoreResult> {
    void mode;
    this.ensureTestMode();
    return { success: false, message: "浏览器测试模式不读取本机 Codex 聊天缓存。请使用 Tauri 桌面版。", importedCount: 0, totalCount: 0, currentSessionCount: 0, historicalConversationCount: 0 };
  }
  async rollbackChatRestore(snapshotId: string): Promise<ChatRestoreResult> {
    void snapshotId;
    this.ensureTestMode();
    return { success: false, message: "浏览器测试模式不支持恢复回滚。请使用 Tauri 桌面版。", importedCount: 0, totalCount: 0, currentSessionCount: 0, historicalConversationCount: 0 };
  }
  async getSettings(): Promise<AppSettings> {
    const stored = localStorage.getItem(this.settingsKey);
    return { ...defaultSettings, ...(stored ? JSON.parse(stored) : {}) };
  }
  async saveSettings(settings: AppSettings) {
    if (localStorage.getItem("provider-deck.e2e.fail-settings-save") === "1") {
      throw new Error("测试后端模拟设置保存失败");
    }
    const resolved: AppSettings = {
      ...settings,
      effectiveReasoningLevel: settings.manualReasoningLevel,
      reasoningMatchMessage: settings.autoReasoningMode ? "浏览器测试模式不读取模型元数据和显存，暂时沿用手动档位" : undefined,
    };
    localStorage.setItem(this.settingsKey, JSON.stringify(resolved));
    return resolved;
  }
  async exportProviders() { return JSON.stringify(await this.listProviders(), null, 2); }
  async importProviders(payload: string) {
    const providers = JSON.parse(payload) as Provider[];
    localStorage.setItem(this.providerKey, JSON.stringify(providers));
    return providers;
  }
  async diagnostics() { return { runtime: "browser-test", platform: navigator.platform, version: "0.1.11" }; }
}

export const backend: AppBackend = isTauri() ? new TauriBackend() : new BrowserBackend();
