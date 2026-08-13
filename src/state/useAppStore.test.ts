import { beforeEach, describe, expect, it, vi } from "vitest";
import type {
  AppSettings,
  ModelInfo,
  ModelReasoningMeta,
  Provider,
  ProviderDraft,
  ReasoningCapability,
  ReasoningTier,
  RuntimeVerification,
  VerificationResult,
} from "../domain/types";
import { defaultSettings } from "../domain/types";
import { normalizeProviderDraft, reasoningMetaKey, useAppStore } from "./useAppStore";

const mocks = vi.hoisted(() => ({ saveSettings: vi.fn(), verifyModelReasoning: vi.fn(), detectModelReasoning: vi.fn() }));

vi.mock("../services/backend", () => ({
  backend: {
    saveSettings: mocks.saveSettings,
    verifyModelReasoning: mocks.verifyModelReasoning,
    detectModelReasoning: mocks.detectModelReasoning,
  },
}));

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
    expect(defaultSettings.manualReasoningLevel).toBe("high");
    expect(defaultSettings.effectiveReasoningLevel).toBe("high");
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
      manualReasoningLevel: "low",
      effectiveReasoningLevel: "medium",
    };
    mocks.saveSettings.mockResolvedValue(settled);

    await useAppStore.getState().updateSettings({ ...defaultSettings, manualReasoningLevel: "low" });

    expect(useAppStore.getState().settings).toEqual(settled);
    expect(useAppStore.getState().operation).toBeUndefined();
  });

  it("修改无关设置时仍然回传推理字段，不会把它们重置为默认值", async () => {
    const stored: AppSettings = { ...defaultSettings, manualReasoningLevel: "low", effectiveReasoningLevel: "low" };
    mocks.saveSettings.mockImplementation(async (settings: AppSettings) => settings);
    useAppStore.setState({ settings: stored });

    await useAppStore.getState().updateSettings({ ...stored, timeoutSeconds: 45 });

    expect(mocks.saveSettings).toHaveBeenCalledWith(expect.objectContaining({
      timeoutSeconds: 45,
      manualReasoningLevel: "low",
      effectiveReasoningLevel: "low",
    }));
    expect(useAppStore.getState().settings.manualReasoningLevel).toBe("low");
    expect(useAppStore.getState().settings.effectiveReasoningLevel).toBe("low");
  });

  it("保存失败时抛出错误并清除操作状态", async () => {
    mocks.saveSettings.mockRejectedValue(new Error("写入失败"));

    await expect(useAppStore.getState().updateSettings(defaultSettings)).rejects.toThrow("写入失败");
    expect(useAppStore.getState().operation).toBeUndefined();
  });
});

// —— verifyModelReasoning 的编排。
//
// 断言集中在两件事：历史只追加不覆盖，以及 capability 一个字节都不变。
// 后者是本轮最容易悄悄破掉的边界——一旦有人为了"顺手"把 Confirmed 写成
// confidence = verified，下面 toEqual(capability) 会立刻失败。

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

const reasoningModel: ModelInfo = {
  id: "test-coder",
  displayName: "test-coder",
  protocol: "openai",
  source: "server",
  capabilities: [],
  reasoning: capability,
};

function provider(overrides: Partial<Provider> = {}): Provider {
  return {
    id: "p1",
    name: "测试服务",
    baseUrl: "https://api.example.test/v1",
    protocol: "openai",
    enabled: true,
    isCurrent: true,
    models: [reasoningModel],
    connectionState: "connected",
    appliedClients: [],
    ...overrides,
  };
}

function verification(tier: ReasoningTier, result: VerificationResult, verifiedAt = "2026-08-12T10:30:00Z"): RuntimeVerification {
  return {
    modelId: "test-coder",
    baseUrl: "https://api.example.test/v1",
    tier,
    binding: { kind: "effort", value: "xhigh" },
    result,
    verifiedAt,
    protocol: "openai",
  };
}

// —— 档位可选面的投影。
//
// 这一组钉住的是**边界**：本 action 只写 reasoningMeta / detectingReasoning 两个字段，
// providers 一个字节都不动，失败也不弹全局错误。

describe("detectModelReasoning", () => {
  const meta: ModelReasoningMeta = {
    supportedProtocols: ["openai"],
    nativeParamKind: "effort-enum",
    matchedCustomTiers: [],
    builtinTiersCompatible: true,
  };

  beforeEach(() => {
    mocks.detectModelReasoning.mockReset();
    useAppStore.setState({
      providers: [provider()], operation: undefined, error: undefined,
      reasoningMeta: {}, detectingReasoning: {},
    });
  });

  it("成功后按 (providerId, modelId) 入缓存，并收尾 detecting 标记", async () => {
    mocks.detectModelReasoning.mockResolvedValue(meta);

    const returned = await useAppStore.getState().detectModelReasoning("p1", "test-coder");

    expect(mocks.detectModelReasoning).toHaveBeenCalledWith("p1", "test-coder");
    expect(returned).toEqual(meta);
    expect(useAppStore.getState().reasoningMeta[reasoningMetaKey("p1", "test-coder")]).toEqual(meta);
    expect(useAppStore.getState().detectingReasoning[reasoningMetaKey("p1", "test-coder")]).toBe(false);
  });

  it("不碰 providers：models 与 reasoningVerifications 一个字节都不动", async () => {
    const before = useAppStore.getState().providers;
    mocks.detectModelReasoning.mockResolvedValue(meta);

    await useAppStore.getState().detectModelReasoning("p1", "test-coder");

    // 同一个引用：本 action 压根没重建 providers 数组。
    expect(useAppStore.getState().providers).toBe(before);
  });

  it("失败时不置全局 error、不清已有缓存，返回 undefined", async () => {
    const key = reasoningMetaKey("p1", "test-coder");
    useAppStore.setState({ reasoningMeta: { [key]: meta } });
    mocks.detectModelReasoning.mockRejectedValue(new Error("boom"));

    const returned = await useAppStore.getState().detectModelReasoning("p1", "test-coder");

    expect(returned).toBeUndefined();
    // 探测失败不代表用户的档位配置没了：上一次的投影原样留着。
    expect(useAppStore.getState().reasoningMeta[key]).toEqual(meta);
    expect(useAppStore.getState().error).toBeUndefined();
    expect(useAppStore.getState().operation).toBeUndefined();
    expect(useAppStore.getState().detectingReasoning[key]).toBe(false);
  });

  it("两个模型的投影互不覆盖", async () => {
    mocks.detectModelReasoning.mockResolvedValueOnce(meta);
    await useAppStore.getState().detectModelReasoning("p1", "test-coder");
    const other: ModelReasoningMeta = { ...meta, nativeParamKind: "token-budget" };
    mocks.detectModelReasoning.mockResolvedValueOnce(other);
    await useAppStore.getState().detectModelReasoning("p1", "other-model");

    expect(useAppStore.getState().reasoningMeta[reasoningMetaKey("p1", "test-coder")]).toEqual(meta);
    expect(useAppStore.getState().reasoningMeta[reasoningMetaKey("p1", "other-model")]).toEqual(other);
  });

  it("任何分隔符都可能出现在 id 里，键仍与两个入参一一对应", () => {
    // 冒号、斜杠、空格都能在 modelId 里合法出现，逐一钉住不许撞车。
    expect(reasoningMetaKey("p1", "vendor:model")).not.toBe(reasoningMetaKey("p1:vendor", "model"));
    expect(reasoningMetaKey("p", "1:vendor:model")).not.toBe(reasoningMetaKey("p1", "vendor:model"));
    expect(reasoningMetaKey("a", "b/c")).not.toBe(reasoningMetaKey("a/b", "c"));
    expect(reasoningMetaKey("a", "b c")).not.toBe(reasoningMetaKey("a b", "c"));
  });

  it("缺 providerId 或 modelId 时不发请求", async () => {
    expect(await useAppStore.getState().detectModelReasoning("", "test-coder")).toBeUndefined();
    expect(await useAppStore.getState().detectModelReasoning("p1", "")).toBeUndefined();
    expect(mocks.detectModelReasoning).not.toHaveBeenCalled();
  });
});

describe("verifyModelReasoning", () => {
  beforeEach(() => {
    mocks.verifyModelReasoning.mockReset();
    useAppStore.setState({ providers: [provider()], operation: undefined, error: undefined });
  });

  it("成功验证后历史长度加一，并按 camelCase 参数调用后端", async () => {
    const record = verification("deep", { status: "confirmed" });
    mocks.verifyModelReasoning.mockResolvedValue(record);

    const returned = await useAppStore.getState().verifyModelReasoning("p1", "test-coder", "deep");

    expect(mocks.verifyModelReasoning).toHaveBeenCalledWith("p1", "test-coder", "deep");
    expect(returned).toEqual(record);
    const history = useAppStore.getState().providers[0]?.reasoningVerifications?.["test-coder"];
    expect(history).toHaveLength(1);
    expect(history?.[0]).toEqual(record);
    expect(useAppStore.getState().operation).toBeUndefined();
    expect(useAppStore.getState().error).toBeUndefined();
  });

  /** 追加语义：history = old + new，绝不是 history = [verification]。 */
  it("重复验证保留多条记录，旧记录原样在前", async () => {
    const first = verification("deep", { status: "confirmed" }, "2026-08-12T10:00:00Z");
    useAppStore.setState({ providers: [provider({ reasoningVerifications: { "test-coder": [first] } })] });
    const second = verification("light", { status: "confirmed" }, "2026-08-12T10:40:00Z");
    mocks.verifyModelReasoning.mockResolvedValue(second);

    await useAppStore.getState().verifyModelReasoning("p1", "test-coder", "light");

    const history = useAppStore.getState().providers[0]?.reasoningVerifications?.["test-coder"];
    expect(history).toHaveLength(2);
    expect(history?.[0]).toEqual(first);
    expect(history?.[1]).toEqual(second);
  });

  /** Rejected ≠ Unsupported：它是一次验证事件，必须留痕，也不该当异常抛。 */
  it("rejected 仍然入库，不抛错、不置 error", async () => {
    const record = verification("deep", { status: "rejected", reason: "响应中未检测到 openai 协议的推理字段" });
    mocks.verifyModelReasoning.mockResolvedValue(record);

    const returned = await useAppStore.getState().verifyModelReasoning("p1", "test-coder", "deep");

    expect(returned.result.status).toBe("rejected");
    expect(useAppStore.getState().providers[0]?.reasoningVerifications?.["test-coder"]).toEqual([record]);
    expect(useAppStore.getState().error).toBeUndefined();
  });

  /** Failed 是"这次请求没走通"的事件记录，同样入库；只有 backend 抛异常才走 error 分支。 */
  it("failed 仍然入库", async () => {
    const record = verification("light", { status: "failed", error: "API 错误 429：rate limited" });
    mocks.verifyModelReasoning.mockResolvedValue(record);

    await useAppStore.getState().verifyModelReasoning("p1", "test-coder", "light");

    const history = useAppStore.getState().providers[0]?.reasoningVerifications?.["test-coder"];
    expect(history).toHaveLength(1);
    expect(history?.[0]?.result).toEqual({ status: "failed", error: "API 错误 429：rate limited" });
  });

  it("三态都不修改 capability / confidence / evidence", async () => {
    const results: VerificationResult[] = [
      { status: "confirmed" },
      { status: "rejected", reason: "无推理字段" },
      { status: "failed", error: "连接被拒绝" },
    ];
    for (const result of results) {
      mocks.verifyModelReasoning.mockResolvedValue(verification("deep", result));
      await useAppStore.getState().verifyModelReasoning("p1", "test-coder", "deep");
      const stored = useAppStore.getState().providers[0]?.models[0]?.reasoning;
      expect(stored).toEqual(capability);
      expect(stored?.confidence).toBe("declared");
      expect(stored?.evidence).toEqual([]);
    }
    // 三次全部留痕，没有任何一次被当成"不值得记录"。
    expect(useAppStore.getState().providers[0]?.reasoningVerifications?.["test-coder"]).toHaveLength(3);
  });

  it("只替换目标 provider，其他 provider 不受影响", async () => {
    const other = provider({ id: "p2", isCurrent: false });
    useAppStore.setState({ providers: [provider(), other] });
    mocks.verifyModelReasoning.mockResolvedValue(verification("deep", { status: "confirmed" }));

    await useAppStore.getState().verifyModelReasoning("p1", "test-coder", "deep");

    expect(useAppStore.getState().providers[1]).toBe(other);
    expect(useAppStore.getState().providers[1]?.reasoningVerifications).toBeUndefined();
  });

  it("后端抛错时置 error、清 operation 并 rethrow，历史不变", async () => {
    mocks.verifyModelReasoning.mockRejectedValue(new Error("无法读取已保存的 API Key"));

    await expect(useAppStore.getState().verifyModelReasoning("p1", "test-coder", "deep"))
      .rejects.toThrow("无法读取已保存的 API Key");
    expect(useAppStore.getState().error).toBe("无法读取已保存的 API Key");
    expect(useAppStore.getState().operation).toBeUndefined();
    expect(useAppStore.getState().providers[0]?.reasoningVerifications).toBeUndefined();
  });
});
