import { beforeEach, describe, expect, it, vi } from "vitest";
import type { AppSettings, ProviderDraft } from "../domain/types";
import { defaultSettings } from "../domain/types";
import { normalizeProviderDraft, useAppStore } from "./useAppStore";

const mocks = vi.hoisted(() => ({ saveSettings: vi.fn() }));

vi.mock("../services/backend", () => ({ backend: { saveSettings: mocks.saveSettings } }));

const draft = (protocolHint: ProviderDraft["protocolHint"]): ProviderDraft => ({
  name: "测试服务",
  baseUrl: "https://api.example.test",
  apiKey: "test-key",
  protocolHint,
  timeoutSeconds: 10,
});

describe("normalizeProviderDraft", () => {
  it("将表单的空协议值转换为未指定协议", () => {
    expect(normalizeProviderDraft(draft("" as ProviderDraft["protocolHint"])).protocolHint).toBeUndefined();
  });

  it("保留明确的协议值并返回新对象", () => {
    const original = draft("anthropic");
    const normalized = normalizeProviderDraft(original);
    expect(normalized).toEqual(original);
    expect(normalized).not.toBe(original);
  });
});

describe("defaultSettings 的推理字段", () => {
  it("与 Rust AppSettings::default 保持一致，避免 serde 回填时重置推理状态", () => {
    expect(defaultSettings.autoReasoningMode).toBe(false);
    expect(defaultSettings.manualReasoningLevel).toBe("high");
    expect(defaultSettings.effectiveReasoningLevel).toBe("high");
    expect(defaultSettings.reasoningMatchMessage).toBeUndefined();
  });
});

describe("updateSettings", () => {
  beforeEach(() => {
    mocks.saveSettings.mockReset();
    useAppStore.setState({ settings: defaultSettings, operation: undefined, error: undefined });
  });

  it("采用后端结算后的推理档位，而不是原样回写草稿", async () => {
    const settled: AppSettings = {
      ...defaultSettings,
      autoReasoningMode: true,
      manualReasoningLevel: "low",
      effectiveReasoningLevel: "medium",
      reasoningMatchMessage: "云端 API，不占用本机显存，自动选用中度推理模式",
    };
    mocks.saveSettings.mockResolvedValue(settled);

    await useAppStore.getState().updateSettings({ ...defaultSettings, autoReasoningMode: true, manualReasoningLevel: "low" });

    expect(useAppStore.getState().settings).toEqual(settled);
    expect(useAppStore.getState().operation).toBeUndefined();
  });

  it("修改无关设置时仍然回传推理字段，不会把它们重置为默认值", async () => {
    const stored: AppSettings = { ...defaultSettings, autoReasoningMode: true, manualReasoningLevel: "low", effectiveReasoningLevel: "low" };
    mocks.saveSettings.mockImplementation(async (settings: AppSettings) => settings);
    useAppStore.setState({ settings: stored });

    await useAppStore.getState().updateSettings({ ...stored, timeoutSeconds: 45 });

    expect(mocks.saveSettings).toHaveBeenCalledWith(expect.objectContaining({
      timeoutSeconds: 45,
      autoReasoningMode: true,
      manualReasoningLevel: "low",
    }));
    expect(useAppStore.getState().settings.autoReasoningMode).toBe(true);
    expect(useAppStore.getState().settings.manualReasoningLevel).toBe("low");
  });

  it("保存失败时抛出错误并清除操作状态", async () => {
    mocks.saveSettings.mockRejectedValue(new Error("写入失败"));

    await expect(useAppStore.getState().updateSettings(defaultSettings)).rejects.toThrow("写入失败");
    expect(useAppStore.getState().operation).toBeUndefined();
  });
});
