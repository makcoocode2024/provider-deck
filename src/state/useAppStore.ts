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
} from "../domain/types";
import { defaultSettings } from "../domain/types";
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
