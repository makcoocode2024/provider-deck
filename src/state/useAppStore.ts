import { create } from "zustand";
import type {
  AppSettings,
  ApplyResult,
  BackupRecord,
  ClientDescriptor,
  ConfigChange,
  ProbeResult,
  Provider,
  ProviderDraft,
  ReasoningTier,
  RuntimeVerification,
} from "../domain/types";
import { defaultSettings } from "../domain/types";
import { appendVerification } from "../domain/reasoning";
import { backend } from "../services/backend";

interface AppState {
  providers: Provider[];
  clients: ClientDescriptor[];
  backups: BackupRecord[];
  settings: AppSettings;
  loading: boolean;
  operation?: string;
  error?: string;
  probeResult?: ProbeResult;
  changes: ConfigChange[];
  applyResults: ApplyResult[];
  hydrate(): Promise<void>;
  probe(draft: ProviderDraft): Promise<ProbeResult>;
  reprobeProvider(id: string): Promise<void>;
  refreshProviderModels(id: string): Promise<Provider>;
  reprobeModelReasoning(providerId: string, modelId: string): Promise<Provider>;
  verifyModelReasoning(providerId: string, modelId: string, tier: ReasoningTier): Promise<RuntimeVerification>;
  testProvider(id: string, modelId?: string): Promise<import("../domain/types").ProviderTestReport>;
  saveProvider(draft: ProviderDraft): Promise<Provider>;
  deleteProvider(id: string): Promise<void>;
  switchProvider(id: string): Promise<void>;
  preview(providerId: string, clientIds: string[]): Promise<void>;
  apply(providerId: string): Promise<void>;
  restore(id: string): Promise<void>;
  updateSettings(settings: AppSettings): Promise<void>;
  clearError(): void;
}

const messageOf = (error: unknown) => error instanceof Error ? error.message : String(error);

export const normalizeProviderDraft = (draft: ProviderDraft): ProviderDraft => ({
  ...draft,
  protocolHint: (draft.protocolHint as string | undefined) === "" ? undefined : draft.protocolHint,
});

export const useAppStore = create<AppState>((set, get) => ({
  providers: [],
  clients: [],
  backups: [],
  settings: defaultSettings,
  loading: true,
  changes: [],
  applyResults: [],

  async hydrate() {
    set({ loading: true, error: undefined });
    try {
      const [providers, clients, backups, settings] = await Promise.all([
        backend.listProviders(), backend.detectClients(), backend.listBackups(), backend.getSettings(),
      ]);
      set({ providers, clients, backups, settings, loading: false });
    } catch (error) {
      set({ loading: false, error: messageOf(error) });
    }
  },

  async probe(draft) {
    set({ operation: "正在探测协议和模型…", error: undefined, probeResult: undefined });
    try {
      const result = await backend.probeProvider(draft);
      set({ operation: undefined, probeResult: result });
      return result;
    } catch (error) {
      set({ operation: undefined, error: messageOf(error) });
      throw error;
    }
  },

  async saveProvider(draft) {
    const normalizedDraft = normalizeProviderDraft(draft);
    const probeResult = get().probeResult ?? await get().probe(normalizedDraft);
    set({ operation: "正在安全保存服务…" });
    try {
      const provider = await backend.saveProvider(normalizedDraft, probeResult);
      const providers = await backend.listProviders();
      set({ providers, operation: undefined });
      return provider;
    } catch (error) {
      set({ operation: undefined, error: messageOf(error) });
      throw error;
    }
  },

  async reprobeProvider(id) {
    set({ operation: "正在使用已保存的凭据重新检测…", error: undefined });
    try {
      const refreshed = await backend.reprobeProvider(id);
      set({
        providers: get().providers.map((provider) => provider.id === refreshed.id ? refreshed : provider),
        operation: undefined,
      });
    } catch (error) {
      set({ operation: undefined, error: messageOf(error) });
    }
  },

  async refreshProviderModels(id) {
    set({ operation: "正在获取服务模型…", error: undefined });
    try {
      const refreshed = await backend.refreshProviderModels(id);
      set({ providers: get().providers.map((provider) => provider.id === refreshed.id ? refreshed : provider), operation: undefined });
      return refreshed;
    } catch (error) {
      set({ operation: undefined, error: messageOf(error) });
      throw error;
    }
  },

  async reprobeModelReasoning(providerId, modelId) {
    set({ operation: "正在重新探测该模型的推理能力…", error: undefined });
    try {
      const refreshed = await backend.reprobeModelReasoning(providerId, modelId);
      // 探测结果同时刷新 probeResult：向导正停在"确认模型"步，能力要立刻反映到选择器上。
      const probeResult = get().probeResult;
      set({
        providers: get().providers.map((provider) => provider.id === refreshed.id ? refreshed : provider),
        probeResult: probeResult ? { ...probeResult, models: refreshed.models } : probeResult,
        operation: undefined,
      });
      return refreshed;
    } catch (error) {
      set({ operation: undefined, error: messageOf(error) });
      throw error;
    }
  },

  /**
   * 运行时验证。本 action 只做编排：调 backend、调 {@link appendVerification}、换掉那一个
   * provider、收尾 operation/error。追加规则本身在 domain 层，store 和组件都不复刻它。
   *
   * 只写 `provider.reasoningVerifications` 一个字段。**不碰** `models`——也就不碰
   * `model.reasoning` 的 confidence / evidence：能力发现与运行时验证是两条并行链路，
   * 一次 Confirmed 不构成探测事实，绝不抬升 confidence 到 verified。
   */
  async verifyModelReasoning(providerId, modelId, tier) {
    set({ operation: "正在向该端点发送一次真实请求以验证推理档位…", error: undefined });
    try {
      const verification = await backend.verifyModelReasoning(providerId, modelId, tier);
      set({
        providers: get().providers.map((provider) => provider.id === providerId
          ? { ...provider, reasoningVerifications: appendVerification(provider.reasoningVerifications, verification) }
          : provider),
        operation: undefined,
      });
      // 三态都返回给调用方：Rejected/Failed 是验证结果，不是异常，组件按 result.status 分支。
      return verification;
    } catch (error) {
      set({ operation: undefined, error: messageOf(error) });
      throw error;
    }
  },

  async testProvider(id, modelId) {
    set({ operation: "正在执行服务自测…", error: undefined });
    try {
      const report = await backend.testProvider(id, modelId);
      set({ operation: undefined });
      return report;
    } catch (error) {
      set({ operation: undefined, error: messageOf(error) });
      throw error;
    }
  },

  async deleteProvider(id) {
    await backend.deleteProvider(id);
    set({ providers: await backend.listProviders() });
  },

  async switchProvider(id) {
    set({ operation: "正在切换当前服务…" });
    try { set({ providers: await backend.setCurrentProvider(id), operation: undefined }); }
    catch (error) { set({ operation: undefined, error: messageOf(error) }); }
  },

  async preview(providerId, clientIds) {
    set({ operation: "正在生成变更预览…", changes: [], applyResults: [] });
    try { set({ changes: await backend.previewChanges(providerId, clientIds), operation: undefined }); }
    catch (error) { set({ operation: undefined, error: messageOf(error) }); }
  },

  async apply(providerId) {
    set({ operation: "正在备份并原子写入…", applyResults: [] });
    try {
      const applyResults = await backend.applyChanges(providerId, get().changes);
      const [providers, backups] = await Promise.all([backend.listProviders(), backend.listBackups()]);
      set({ applyResults, providers, backups, operation: undefined });
    } catch (error) { set({ operation: undefined, error: messageOf(error) }); }
  },

  async restore(id) {
    set({ operation: "正在校验并恢复备份…" });
    try { await backend.restoreBackup(id); set({ backups: await backend.listBackups(), operation: undefined }); }
    catch (error) { set({ operation: undefined, error: messageOf(error) }); }
  },

  async updateSettings(settings) {
    set({ operation: "正在保存设置…", error: undefined });
    try {
      set({ settings: await backend.saveSettings(settings), operation: undefined });
    } catch (error) {
      set({ operation: undefined });
      throw error;
    }
  },

  clearError() { set({ error: undefined }); },
}));
