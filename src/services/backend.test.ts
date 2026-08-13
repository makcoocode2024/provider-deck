// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ModelInfo, ModelReasoningMeta, Provider, ReasoningCapability, RuntimeVerification } from "../domain/types";
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
    expect(settings.manualReasoningLevel).toBe(defaultSettings.manualReasoningLevel);
    expect(settings.effectiveReasoningLevel).toBe(defaultSettings.effectiveReasoningLevel);
  });

  it("把生效档位结算为所选的回退档位并原样返回", async () => {
    const saved = await backend.saveSettings({
      ...defaultSettings,
      timeoutSeconds: 45,
      manualReasoningLevel: "medium",
    });

    expect(saved.timeoutSeconds).toBe(45);
    expect(saved.manualReasoningLevel).toBe("medium");
    expect(saved.effectiveReasoningLevel).toBe("medium");
    expect((await backend.getSettings()).manualReasoningLevel).toBe("medium");
  });

  it("旧数据没有兜底表、规则表、档位表时都补成空数组", async () => {
    localStorage.setItem(settingsKey, JSON.stringify({ timeoutSeconds: 20, manualReasoningLevel: "low" }));
    const settings = await backend.getSettings();
    expect(settings.reasoningFallbacks).toEqual([]);
    expect(settings.reasoningNameRules).toEqual([]);
    expect(settings.customReasoningTiers).toEqual([]);
  });

  it("旧兜底表的 level 读取时迁移成 tierId，与 Rust 侧同一套映射", async () => {
    localStorage.setItem(settingsKey, JSON.stringify({
      manualReasoningLevel: "low",
      reasoningFallbacks: [
        { modelId: "a", level: "low" },
        { modelId: "b", level: "medium" },
        { modelId: "c", level: "high" },
        // 两个字段同时存在时 tierId 胜出：那是新版写出的值，level 只是残留。
        { modelId: "d", level: "low", tierId: "max" },
      ],
    }));

    expect((await backend.getSettings()).reasoningFallbacks).toEqual([
      { modelId: "a", tierId: "light" },
      { modelId: "b", tierId: "standard" },
      { modelId: "c", tierId: "deep" },
      { modelId: "d", tierId: "max" },
    ]);
  });

  it("兜底表按后端同一套规则归一化后存盘", async () => {
    const saved = await backend.saveSettings({
      ...defaultSettings,
      reasoningFallbacks: [
        { modelId: " padded-model ", tierId: "light" },
        { modelId: "", tierId: "deep" },
        { modelId: "dup", tierId: "light" },
        { modelId: "dup", tierId: "deep" },
      ],
    });

    expect(saved.reasoningFallbacks).toEqual([
      { modelId: "padded-model", tierId: "light" },
      { modelId: "dup", tierId: "deep" },
    ]);
    expect((await backend.getSettings()).reasoningFallbacks).toEqual(saved.reasoningFallbacks);
  });

  it("指向已删除档位的兜底记录照样存盘，不在保存时被抹掉", async () => {
    // 抹掉等于用户重建同名档位后兜底不会自动恢复。悬空引用交给结算时降级。
    const saved = await backend.saveSettings({
      ...defaultSettings,
      reasoningFallbacks: [{ modelId: "glm-4-plus", tierId: "deleted-tier" }],
      reasoningNameRules: [{ id: "r", pattern: "glm-", matchType: "prefix", tierId: "also-deleted" }],
    });

    expect(saved.reasoningFallbacks).toEqual([{ modelId: "glm-4-plus", tierId: "deleted-tier" }]);
    expect(saved.reasoningNameRules).toEqual([
      { id: "r", pattern: "glm-", matchType: "prefix", tierId: "also-deleted" },
    ]);
  });

  it("规则表保留顺序和重复，只丢掉不可能命中的空行", async () => {
    const saved = await backend.saveSettings({
      ...defaultSettings,
      reasoningNameRules: [
        { id: "r1", pattern: " glm- ", matchType: "prefix", tierId: " deep " },
        { id: "r2", pattern: "   ", matchType: "contains", tierId: "light" },
        { id: "r3", pattern: "thinking", matchType: "contains", tierId: "  " },
        { id: "r4", pattern: "glm-", matchType: "prefix", tierId: "light" },
      ],
    });

    // r1 与 r4 是同一个 pattern，但顺序即优先级，去重会改变用户表达的意图。
    expect(saved.reasoningNameRules).toEqual([
      { id: "r1", pattern: "glm-", matchType: "prefix", tierId: "deep" },
      { id: "r4", pattern: "glm-", matchType: "prefix", tierId: "light" },
    ]);
    expect((await backend.getSettings()).reasoningNameRules).toEqual(saved.reasoningNameRules);
  });

  it("自定义档位只 trim 名字，协议参数原样保存", async () => {
    const params = { reasoning: { effort: "xhigh" } };
    const saved = await backend.saveSettings({
      ...defaultSettings,
      customReasoningTiers: [
        { id: " tier-x ", label: " 超深 ", openaiParams: params },
        { id: "   ", label: "无 id", openaiParams: params },
      ],
    });

    // 本项目不维护各家网关的参数字典，所以参数内容一个字都不改写。
    expect(saved.customReasoningTiers).toEqual([{ id: "tier-x", label: "超深", openaiParams: params }]);
    expect((await backend.getSettings()).customReasoningTiers).toEqual(saved.customReasoningTiers);
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

describe("TauriBackend.detectModelReasoning 的 invoke 契约", () => {
  afterEach(() => {
    delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
    mocks.invoke.mockReset();
    vi.resetModules();
  });

  it("按 camelCase 传 providerId / modelId，command 名为 detect_model_reasoning", async () => {
    (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {};
    const returned: ModelReasoningMeta = {
      supportedProtocols: ["openai"],
      nativeParamKind: "effort-enum",
      matchedCustomTiers: [],
      builtinTiersCompatible: true,
    };
    mocks.invoke.mockResolvedValue(returned);

    vi.resetModules();
    const { backend: tauriBackend } = await import("./backend");
    const meta = await tauriBackend.detectModelReasoning("p1", "test-coder");

    expect(mocks.invoke).toHaveBeenCalledWith("detect_model_reasoning", {
      providerId: "p1",
      modelId: "test-coder",
    });
    // snake_case 会让 Tauri 报参数缺失，这里正面钉住不许出现。
    const [, payload] = mocks.invoke.mock.calls[0] as [string, Record<string, unknown>];
    expect(Object.keys(payload)).toEqual(["providerId", "modelId"]);
    expect(payload).not.toHaveProperty("provider_id");
    expect(payload).not.toHaveProperty("model_id");
    expect(meta).toEqual(returned);
  });
});

describe("BrowserBackend.detectModelReasoning", () => {
  beforeEach(() => localStorage.clear());

  it("匹配段来自名称规则，且按规则表顺序去重", async () => {
    const { backend: browserBackend } = await import("./backend");
    const provider = seedProvider();
    await browserBackend.saveSettings({
      ...await browserBackend.getSettings(),
      customReasoningTiers: [
        { id: "tier-x", label: "超深", openaiParams: { reasoning: { effort: "xhigh" } } },
        { id: "tier-y", label: "极限", anthropicParams: { thinking: { budget_tokens: 4096 } } },
      ],
      reasoningNameRules: [
        { id: "r1", pattern: "test-", matchType: "prefix", tierId: "tier-x" },
        { id: "r2", pattern: "coder", matchType: "contains", tierId: "tier-y" },
        // 同一档位命中两次只出现一次：下拉里出现两个同名项是 bug，不是信息。
        { id: "r3", pattern: "test", matchType: "contains", tierId: "tier-x" },
        // 指向已删除的档位：整条跳过，不产出空项。
        { id: "r4", pattern: "test", matchType: "contains", tierId: "gone" },
      ],
    });

    const meta = await browserBackend.detectModelReasoning(provider.id, "test-coder");
    expect(meta.matchedCustomTiers.map((tier) => tier.tierId)).toEqual(["tier-x", "tier-y"]);
    expect(meta.matchedCustomTiers[0]).toMatchObject({ label: "超深", rulePattern: "test-", ruleMatchType: "prefix" });
    // OpenAI 系三个协议共用一份参数，一并列出——只报 openai 会让 Azure 端点上的用户误判。
    expect(meta.matchedCustomTiers[0].supportedProtocols).toEqual(["openai", "azure-openai", "custom"]);
    expect(meta.matchedCustomTiers[1].supportedProtocols).toEqual(["anthropic"]);
  });

  it("已探明 effortEnum：形态与内置档位可用性都如实投影", async () => {
    const { backend: browserBackend } = await import("./backend");
    const provider = seedProvider();
    const meta = await browserBackend.detectModelReasoning(provider.id, "test-coder");
    expect(meta.nativeParamKind).toBe("effort-enum");
    expect(meta.builtinTiersCompatible).toBe(true);
    expect(meta.supportedProtocols).toEqual(["openai"]);
  });

  it("模型未探测时形态为 unknown，且不断言内置档位可用", async () => {
    const { backend: browserBackend } = await import("./backend");
    const provider = seedProvider({ models: [{ ...model, reasoning: undefined }] });
    const meta = await browserBackend.detectModelReasoning(provider.id, "test-coder");
    expect(meta.nativeParamKind).toBe("unknown");
    // null 是"无法确认"，不是 false。写成 false 就是把未探明伪装成探测结论。
    expect(meta.builtinTiersCompatible ?? null).toBeNull();
    // 形态未知时不报协议：报了会让界面以为档位在这个端点一定写得出参数。
    expect(meta.supportedProtocols).toEqual([]);
  });

  it("已探到不支持：builtinTiersCompatible 为 false，与 unknown 分得开", async () => {
    const { backend: browserBackend } = await import("./backend");
    const unsupported = { ...capability, support: "unsupported" as const, tiers: [], control: { kind: "none" as const } };
    const provider = seedProvider({ models: [{ ...model, reasoning: unsupported }] });
    const meta = await browserBackend.detectModelReasoning(provider.id, "test-coder");
    expect(meta.builtinTiersCompatible).toBe(false);
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
