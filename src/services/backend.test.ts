// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ModelInfo, Provider, ReasoningCapability, RuntimeVerification } from "../domain/types";
import { defaultSettings } from "../domain/types";
import { backend } from "./backend";

const mocks = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: mocks.invoke }));

const settingsKey = "provider-deck.e2e.settings";
const providerKey = "provider-deck.e2e.providers";

describe("浏览器测试后端的设置读写", () => {
  beforeEach(() => localStorage.clear());

  it("读取缺少推理字段的旧数据时补齐默认值", async () => {
    localStorage.setItem(settingsKey, JSON.stringify({
      timeoutSeconds: 20,
      proxyUrl: "",
      allowSelfSignedCertificates: false,
      generateOnly: true,
      clearClipboardSeconds: 30,
      locale: "zh-CN",
    }));

    const settings = await backend.getSettings();

    expect(settings.timeoutSeconds).toBe(20);
    expect(settings.autoReasoningMode).toBe(defaultSettings.autoReasoningMode);
    expect(settings.manualReasoningLevel).toBe(defaultSettings.manualReasoningLevel);
    expect(settings.effectiveReasoningLevel).toBe(defaultSettings.effectiveReasoningLevel);
  });

  it("手动模式下把生效档位结算为所选档位并原样返回", async () => {
    const saved = await backend.saveSettings({
      ...defaultSettings,
      timeoutSeconds: 45,
      autoReasoningMode: false,
      manualReasoningLevel: "medium",
    });

    expect(saved.timeoutSeconds).toBe(45);
    expect(saved.manualReasoningLevel).toBe("medium");
    expect(saved.effectiveReasoningLevel).toBe("medium");
    expect(saved.reasoningMatchMessage).toBeUndefined();
    expect((await backend.getSettings()).manualReasoningLevel).toBe("medium");
  });

  it("自动模式下保留手动档位并说明测试后端不做真实推荐", async () => {
    const saved = await backend.saveSettings({ ...defaultSettings, autoReasoningMode: true, manualReasoningLevel: "low" });

    expect(saved.autoReasoningMode).toBe(true);
    expect(saved.manualReasoningLevel).toBe("low");
    expect(saved.reasoningMatchMessage).toContain("浏览器测试模式");
  });
});

// —— 运行时验证的两套后端。
//
// `backend` 这个导出在模块加载时就按 `__TAURI_INTERNALS__` 二选一定型了，
// 而 Vitest 里没有这个全局，拿到的必然是 BrowserBackend。要断言 invoke 的参数名，
// 只能重新 import 一次模块并临时装上该全局——这正是本段动态 import 的理由。

const capability: ReasoningCapability = {
  key: { baseUrl: "https://api.example.test/v1", modelId: "test-coder" },
  support: "supported",
  control: { kind: "effortEnum", values: ["minimal", "medium", "xhigh"] },
  tiers: [
    { tier: "light", id: "light", label: "轻度", binding: { kind: "effort", value: "minimal" }, wireSummary: "effort=minimal" },
    { tier: "deep", id: "deep", label: "高", binding: { kind: "effort", value: "xhigh" }, wireSummary: "effort=xhigh" },
  ],
  defaultTier: "deep",
  constraints: {},
  confidence: "declared",
  evidence: [],
  discoveredAt: "2026-08-12T10:00:00Z",
  ttlSeconds: 14 * 24 * 3600,
};

const model: ModelInfo = {
  id: "test-coder",
  displayName: "test-coder",
  protocol: "openai",
  source: "server",
  capabilities: [],
  reasoning: capability,
};

function seedProvider(overrides: Partial<Provider> = {}): Provider {
  const provider: Provider = {
    id: "p1",
    name: "测试服务",
    baseUrl: "https://api.example.test/v1",
    protocol: "openai",
    enabled: true,
    isCurrent: true,
    models: [model],
    connectionState: "connected",
    appliedClients: [],
    ...overrides,
  };
  localStorage.setItem(providerKey, JSON.stringify([provider]));
  return provider;
}

describe("TauriBackend.verifyModelReasoning 的 invoke 契约", () => {
  afterEach(() => {
    delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
    mocks.invoke.mockReset();
    vi.resetModules();
  });

  it("按 camelCase 传 providerId / modelId / tier，command 名为 verify_model_reasoning", async () => {
    (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {};
    const returned: RuntimeVerification = {
      modelId: "test-coder",
      baseUrl: "https://api.example.test/v1",
      tier: "deep",
      binding: { kind: "effort", value: "xhigh" },
      result: { status: "confirmed" },
      verifiedAt: "2026-08-12T10:30:00Z",
      protocol: "openai",
    };
    mocks.invoke.mockResolvedValue(returned);

    vi.resetModules();
    const { backend: tauriBackend } = await import("./backend");
    const verification = await tauriBackend.verifyModelReasoning("p1", "test-coder", "deep");

    expect(mocks.invoke).toHaveBeenCalledWith("verify_model_reasoning", {
      providerId: "p1",
      modelId: "test-coder",
      tier: "deep",
    });
    // snake_case 会让 Tauri 报参数缺失，这里正面钉住不许出现。
    const [, payload] = mocks.invoke.mock.calls[0] as [string, Record<string, unknown>];
    expect(Object.keys(payload)).toEqual(["providerId", "modelId", "tier"]);
    expect(payload).not.toHaveProperty("provider_id");
    expect(payload).not.toHaveProperty("model_id");
    expect(verification).toEqual(returned);
  });
});

describe("BrowserBackend.verifyModelReasoning", () => {
  beforeEach(() => localStorage.clear());

  /** 字段名逐一核对：后端 Provider 是 camelCase serde，前端拿到的必须是这套键。 */
  it("返回符合 RuntimeVerification 契约的 camelCase 记录，binding 取自能力表", async () => {
    seedProvider();

    const verification = await backend.verifyModelReasoning("p1", "test-coder", "deep");

    expect(Object.keys(verification).sort()).toEqual(
      ["baseUrl", "binding", "modelId", "protocol", "result", "tier", "verifiedAt"],
    );
    expect(verification.modelId).toBe("test-coder");
    expect(verification.baseUrl).toBe("https://api.example.test/v1");
    expect(verification.tier).toBe("deep");
    // 不是调用方自带的绑定，而是能力表里 deep 那一档的绑定。
    expect(verification.binding).toEqual({ kind: "effort", value: "xhigh" });
    expect(verification.result).toEqual({ status: "confirmed" });
    expect(verification.protocol).toBe("openai");
    expect(Number.isNaN(Date.parse(verification.verifiedAt))).toBe(false);
  });

  it("三态可切换，rejected / failed 的文案与后端同格式", async () => {
    seedProvider();

    localStorage.setItem("provider-deck.e2e.verify-result", "rejected");
    const rejected = await backend.verifyModelReasoning("p1", "test-coder", "light");
    expect(rejected.result).toEqual({ status: "rejected", reason: "响应中未检测到 openai 协议的推理字段" });

    localStorage.setItem("provider-deck.e2e.verify-result", "failed");
    const failed = await backend.verifyModelReasoning("p1", "test-coder", "light");
    expect(failed.result.status).toBe("failed");
  });

  it("三态一律入库且不覆盖历史，同时不触碰 model.reasoning", async () => {
    seedProvider();

    await backend.verifyModelReasoning("p1", "test-coder", "deep");
    localStorage.setItem("provider-deck.e2e.verify-result", "failed");
    await backend.verifyModelReasoning("p1", "test-coder", "light");

    const [stored] = await backend.listProviders();
    expect(stored.reasoningVerifications?.["test-coder"]).toHaveLength(2);
    expect(stored.reasoningVerifications?.["test-coder"]?.[0]?.result.status).toBe("confirmed");
    expect(stored.reasoningVerifications?.["test-coder"]?.[1]?.result.status).toBe("failed");
    // 能力对象逐字段不变：confidence 仍是 declared，evidence 仍为空。
    expect(stored.models[0]?.reasoning).toEqual(capability);
  });

  it("模型不属于该服务、能力未探明、档位不存在时分别报错", async () => {
    seedProvider();
    await expect(backend.verifyModelReasoning("p1", "other", "deep")).rejects.toThrow("不属于该服务");
    await expect(backend.verifyModelReasoning("p1", "test-coder", "max")).rejects.toThrow("不存在");

    seedProvider({ models: [{ ...model, reasoning: undefined }] });
    await expect(backend.verifyModelReasoning("p1", "test-coder", "deep")).rejects.toThrow("尚未探明");
  });
});
