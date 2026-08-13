// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type {
  AppSettings,
  MatchedCustomTier,
  ModelInfo,
  ModelReasoningMeta,
  ProbeResult,
  Provider,
  ReasoningCapability,
  RuntimeVerification,
} from "../domain/types";
import { defaultSettings } from "../domain/types";

// 向导会在打开时读密钥、在保存时调 store，这里把两层都换成 mock：
// 本文件只关心「验证入口的门控」和「验证历史的数据源」，不触碰探测/保存流程。
const mocks = vi.hoisted(() => ({
  getProviderApiKey: vi.fn(),
  verifyModelReasoning: vi.fn(),
  reprobeModelReasoning: vi.fn(),
  detectModelReasoning: vi.fn(),
  updateSettings: vi.fn(),
  probe: vi.fn(),
  saveProvider: vi.fn(),
  clearError: vi.fn(),
}));

vi.mock("../services/backend", () => ({
  backend: { getProviderApiKey: mocks.getProviderApiKey },
}));

const capability: ReasoningCapability = {
  key: { baseUrl: "https://api.example.com/v1", modelId: "test-coder" },
  support: "supported",
  control: { kind: "effortEnum", values: ["minimal", "medium", "xhigh"] },
  tiers: [
    { tier: "light", id: "light", label: "轻度推理", binding: { kind: "effort", value: "minimal" }, wireSummary: "reasoning.effort = minimal" },
    { tier: "standard", id: "standard", label: "标准推理", binding: { kind: "effort", value: "medium" }, wireSummary: "reasoning.effort = medium" },
  ],
  defaultTier: "standard",
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

const probeResult: ProbeResult = {
  normalizedBaseUrl: "https://api.example.com/v1",
  protocol: "openai",
  confidence: 0.94,
  models: [model],
  checkedEndpoints: ["https://api.example.com/v1/models"],
  userMessage: "已识别服务",
};

function verification(status: "confirmed" | "rejected"): RuntimeVerification {
  return {
    modelId: "test-coder",
    baseUrl: "https://api.example.com/v1",
    tier: "standard",
    binding: { kind: "effort", value: "medium" },
    result: status === "confirmed" ? { status: "confirmed" } : { status: "rejected", reason: "无推理字段" },
    verifiedAt: "2026-08-12T10:30:00Z",
    protocol: "openai",
  };
}

function savedProvider(verifications?: Record<string, RuntimeVerification[]>): Provider {
  return {
    id: "p1",
    name: "测试服务",
    baseUrl: "https://api.example.com/v1",
    protocol: "openai",
    enabled: true,
    isCurrent: true,
    defaultModel: "test-coder",
    models: [model],
    connectionState: "connected",
    appliedClients: [],
    reasoningVerifications: verifications,
  };
}

/** 把 store 置成"向导已停在确认模型步"的状态，省去跑一遍真实探测。 */
function seedStore(providers: Provider[], overrides: Partial<AppSettings> = {}, result = probeResult) {
  useAppStore.setState({
    providers,
    clients: [],
    backups: [],
    settings: { ...defaultSettings, ...overrides },
    loading: false,
    operation: undefined,
    error: undefined,
    probeResult: result,
    changes: [],
    applyResults: [],
    reasoningMeta: {},
    detectingReasoning: {},
    probe: mocks.probe,
    saveProvider: mocks.saveProvider,
    reprobeModelReasoning: mocks.reprobeModelReasoning,
    detectModelReasoning: mocks.detectModelReasoning,
    updateSettings: mocks.updateSettings,
    verifyModelReasoning: mocks.verifyModelReasoning,
    clearError: mocks.clearError,
  });
}

import { useAppStore } from "../state/useAppStore";
import { ProviderWizard } from "./ProviderWizard";

/**
 * 把向导推进到"确认模型"步。
 *
 * 只有走一遍 `runProbe` 才会切到 models 步（step 是组件内部 state，没有外部入口），
 * 所以这里填完表单点检测；`probe` 已被换成返回固定 probeResult 的 mock，不发真实请求。
 */
async function openAtModelsStep(initial?: Provider) {
  render(<ProviderWizard open initial={initial} onOpenChange={vi.fn()} onSaved={vi.fn()} />);

  if (initial?.id) {
    // 编辑流程会异步读密钥，等它落到输入框再点，否则 zod 会因 apiKey 为空拦下来。
    await waitFor(() => expect(screen.getByText(/已从系统凭据库读取/)).toBeInTheDocument());
  } else {
    fireEvent.change(screen.getByPlaceholderText("例如：公司开发服务"), { target: { value: "新建服务" } });
    fireEvent.change(screen.getByPlaceholderText("https://api.example.com/v1"), { target: { value: "https://api.example.com/v1" } });
    fireEvent.change(screen.getByPlaceholderText("仅用于目标服务和本机配置"), { target: { value: "test-key" } });
  }

  fireEvent.click(screen.getByRole("button", { name: /检测/ }));
  await screen.findByLabelText("推理档位");
}

describe("ProviderWizard 的验证入口门控", () => {
  beforeEach(() => {
    for (const mock of Object.values(mocks)) mock.mockReset();
    mocks.getProviderApiKey.mockResolvedValue("stored-key");
    // 向导 await 的是这个返回值，不是 store 里的 probeResult 字段。
    mocks.probe.mockResolvedValue(probeResult);
  });

  afterEach(() => cleanup());

  it("编辑已保存服务时把 provider.reasoningVerifications 传进选择器", async () => {
    seedStore([savedProvider({ "test-coder": [verification("confirmed")] })]);

    await openAtModelsStep(savedProvider({ "test-coder": [verification("confirmed")] }));

    expect(await screen.findByLabelText("运行时验证")).toBeInTheDocument();
    // 徽章文案来自 store 里那条记录，证明历史确实流到了组件。
    expect(screen.getByText("已验证 标准推理")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /验证「标准推理」档位/ })).toBeInTheDocument();
  });

  it("历史来自 store 中的 provider 而不是传入的 initial 快照", async () => {
    // initial 是打开向导那一刻的快照（无历史），store 里已经有一条 rejected：
    // 组件必须读 store，否则用户点完验证看不到刚产生的结果。
    seedStore([savedProvider({ "test-coder": [verification("rejected")] })]);

    await openAtModelsStep(savedProvider(undefined));

    expect(await screen.findByText(/此 endpoint 下「标准推理」未检测到推理产物/)).toBeInTheDocument();
    expect(screen.getByText(/无推理字段/)).toBeInTheDocument();
    expect(document.body.textContent).not.toContain("不支持");
  });

  it("新建流程没有 provider id 时不显示验证入口", async () => {
    seedStore([]);

    await openAtModelsStep(undefined);

    // 档位选择器仍在（能力来自 probeResult），只是验证区整块缺席。
    expect(await screen.findByLabelText("推理档位")).toBeInTheDocument();
    expect(screen.queryByLabelText("运行时验证")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /验证/ })).not.toBeInTheDocument();
  });

  it("没有验证历史的已保存服务显示尚未验证，入口仍在", async () => {
    seedStore([savedProvider(undefined)]);

    await openAtModelsStep(savedProvider(undefined));

    expect(await screen.findByText("尚未验证")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /验证「标准推理」档位/ })).toBeEnabled();
  });
});

// —— 从模型卡片新建自定义档位。
//
// 这一组的共同前提：模型能力**未探明**（那才有新建入口），且弹窗与设置页共用一个组件。
// 断言分三类：预填是否可改写、保存后是否落盘并重探、以及自动选中的三条边界。

const unknownModel: ModelInfo = { ...model, reasoning: { ...capability, support: "unknown", tiers: [], confidence: "unknown" } };
const unknownProbe: ProbeResult = { ...probeResult, models: [unknownModel] };

function unknownProvider(): Provider {
  return { ...savedProvider(undefined), models: [unknownModel] };
}

/**
 * 后端返回"刚存的那个档位命中了当前模型且当前协议有参数"。
 *
 * 档位 id 由组件按时间戳生成，测试拿不到，所以从第一次 `updateSettings` 的入参里取——
 * 这也顺带钉住了"重探必须发生在落盘之后"：取不到 id 就说明顺序错了。
 */
function matchedMeta(overrides: Partial<MatchedCustomTier> = {}): ModelReasoningMeta {
  const [saved] = (mocks.updateSettings.mock.calls[0] ?? [undefined]) as [AppSettings | undefined];
  return {
    supportedProtocols: ["openai"],
    nativeParamKind: "unknown",
    matchedCustomTiers: [{
      tierId: saved?.customReasoningTiers[0]?.id ?? "missing",
      label: "超深",
      rulePattern: "test-coder",
      ruleMatchType: "prefix",
      supportedProtocols: ["openai", "azure-openai", "custom"],
      ...overrides,
    }],
  };
}

async function openTierDialogAt(initial: Provider, settings: Partial<AppSettings> = {}) {
  seedStore([initial], settings, unknownProbe);
  await openAtModelsStep(initial);
  fireEvent.click(await screen.findByRole("button", { name: /新建自定义档位/ }));
  return await screen.findByLabelText("自定义推理档位");
}

/** 填完最小可保存内容并提交。`pattern` 传 `null` 表示把预填的规则清空。 */
function fillAndSaveTier(pattern?: string | null) {
  fireEvent.change(screen.getByLabelText("档位名称"), { target: { value: "超深" } });
  fireEvent.change(screen.getByLabelText("OpenAI 协议参数"), { target: { value: '{"reasoning":{"effort":"xhigh"}}' } });
  if (pattern !== undefined) {
    fireEvent.change(screen.getByLabelText("模型名匹配规则"), { target: { value: pattern ?? "" } });
  }
  fireEvent.click(screen.getByRole("button", { name: /保存档位/ }));
}

describe("ProviderWizard 从模型卡片新建档位", () => {
  beforeEach(() => {
    for (const mock of Object.values(mocks)) mock.mockReset();
    mocks.getProviderApiKey.mockResolvedValue("stored-key");
    mocks.probe.mockResolvedValue(unknownProbe);
    mocks.updateSettings.mockResolvedValue(undefined);
    mocks.detectModelReasoning.mockResolvedValue(undefined);
  });

  afterEach(() => cleanup());

  it("弹窗以当前模型名预填前缀规则，且复用设置页的同一个组件", async () => {
    await openTierDialogAt(unknownProvider());
    expect((screen.getByLabelText("模型名匹配规则") as HTMLInputElement).value).toBe("test-coder");
    expect((screen.getByLabelText("匹配方式") as HTMLSelectElement).value).toBe("prefix");
    // 三个协议参数框是设置页那份弹窗的标志，证明没有另造一个组件。
    expect(screen.getByLabelText("Anthropic 协议参数")).toBeInTheDocument();
    expect(screen.getByLabelText("Gemini 协议参数")).toBeInTheDocument();
  });

  it("预填规则可被改写：存下去的是用户改后的写法", async () => {
    mocks.detectModelReasoning.mockImplementation(() => Promise.resolve(matchedMeta()));
    await openTierDialogAt(unknownProvider());
    fillAndSaveTier("test-");

    await waitFor(() => expect(mocks.updateSettings).toHaveBeenCalled());
    const [saved] = mocks.updateSettings.mock.calls[0] as [AppSettings];
    expect(saved.reasoningNameRules).toHaveLength(1);
    expect(saved.reasoningNameRules[0]).toMatchObject({ pattern: "test-", matchType: "prefix" });
    expect(saved.customReasoningTiers[0]).toMatchObject({ label: "超深", openaiParams: { reasoning: { effort: "xhigh" } } });
  });

  it("预填规则被清空后仍能保存：只建档位、不建规则", async () => {
    await openTierDialogAt(unknownProvider());
    fillAndSaveTier(null);

    await waitFor(() => expect(mocks.updateSettings).toHaveBeenCalled());
    const [saved] = mocks.updateSettings.mock.calls[0] as [AppSettings];
    // 空 pattern 会命中一切模型，绝不能存下去。
    expect(saved.reasoningNameRules).toHaveLength(0);
    expect(saved.customReasoningTiers).toHaveLength(1);
  });

  it("保存后重探投影；新档位适配当前模型时写成该模型的兜底档位", async () => {
    mocks.detectModelReasoning.mockImplementation(() => Promise.resolve(matchedMeta()));
    await openTierDialogAt(unknownProvider());
    fillAndSaveTier();

    await waitFor(() => expect(mocks.updateSettings).toHaveBeenCalledTimes(2));
    expect(mocks.detectModelReasoning).toHaveBeenCalledWith("p1", "test-coder");
    const [second] = mocks.updateSettings.mock.calls[1] as [AppSettings];
    expect(second.reasoningFallbacks).toEqual([{ modelId: "test-coder", tierId: second.customReasoningTiers[0].id }]);
  });

  it("规则没命中当前模型时保持原选择，不写兜底表", async () => {
    // 后端返回的匹配清单里没有这个新档位 —— 规则没命中。
    mocks.detectModelReasoning.mockResolvedValue({ supportedProtocols: ["openai"], nativeParamKind: "unknown", matchedCustomTiers: [] } satisfies ModelReasoningMeta);
    await openTierDialogAt(unknownProvider());
    fillAndSaveTier();

    await waitFor(() => expect(mocks.detectModelReasoning).toHaveBeenCalled());
    // 只有第一次落盘，没有第二次"选中"写入。
    expect(mocks.updateSettings).toHaveBeenCalledTimes(1);
  });

  it("档位在当前协议没参数时不自动选中：写不出参数就不算生效", async () => {
    mocks.detectModelReasoning.mockImplementation(() => Promise.resolve(matchedMeta({ supportedProtocols: ["anthropic"] })));
    await openTierDialogAt(unknownProvider());
    fillAndSaveTier();

    await waitFor(() => expect(mocks.detectModelReasoning).toHaveBeenCalled());
    expect(mocks.updateSettings).toHaveBeenCalledTimes(1);
  });

  it("自动选中只作用于当前模型：其他模型的既有兜底原样留着", async () => {
    mocks.detectModelReasoning.mockImplementation(() => Promise.resolve(matchedMeta()));
    await openTierDialogAt(unknownProvider(), {
      reasoningFallbacks: [{ modelId: "other-model", tierId: "light" }],
    });
    fillAndSaveTier();

    await waitFor(() => expect(mocks.updateSettings).toHaveBeenCalledTimes(2));
    const [second] = mocks.updateSettings.mock.calls[1] as [AppSettings];
    expect(second.reasoningFallbacks).toContainEqual({ modelId: "other-model", tierId: "light" });
    expect(second.reasoningFallbacks).toHaveLength(2);
  });

  it("同一模型已有兜底时覆盖那一条，不追加重复项", async () => {
    mocks.detectModelReasoning.mockImplementation(() => Promise.resolve(matchedMeta()));
    await openTierDialogAt(unknownProvider(), {
      reasoningFallbacks: [{ modelId: "test-coder", tierId: "light" }],
    });
    fillAndSaveTier();

    await waitFor(() => expect(mocks.updateSettings).toHaveBeenCalledTimes(2));
    const [second] = mocks.updateSettings.mock.calls[1] as [AppSettings];
    expect(second.reasoningFallbacks).toHaveLength(1);
    expect(second.reasoningFallbacks[0].tierId).not.toBe("light");
  });

  it("落盘失败时留在弹窗里报错，不重探、不选中，报错不含密钥", async () => {
    mocks.updateSettings.mockRejectedValue(new Error("磁盘只读"));
    await openTierDialogAt(unknownProvider());
    // 进入"确认模型"步时已经投影过一次，这里只关心保存失败后有没有再探一次。
    const detectsBefore = mocks.detectModelReasoning.mock.calls.length;
    fillAndSaveTier();

    expect(await screen.findByText(/档位未能保存：磁盘只读/)).toBeInTheDocument();
    expect(screen.getByLabelText("自定义推理档位")).toBeInTheDocument();
    expect(mocks.detectModelReasoning.mock.calls.length).toBe(detectsBefore);
    expect(mocks.updateSettings).toHaveBeenCalledTimes(1);
    expect(document.body.textContent).not.toContain("stored-key");
  });

  it("新建档位不改动置信度标签：能力探测与用户设定是两条链路", async () => {
    mocks.detectModelReasoning.mockImplementation(() => Promise.resolve(matchedMeta()));
    await openTierDialogAt(unknownProvider());
    const before = screen.getByText("能力未探明").textContent;
    fillAndSaveTier();

    await waitFor(() => expect(mocks.updateSettings).toHaveBeenCalledTimes(2));
    expect(screen.getByText("能力未探明").textContent).toBe(before);
    expect(document.body.textContent).not.toContain("不支持推理");
  });
});
