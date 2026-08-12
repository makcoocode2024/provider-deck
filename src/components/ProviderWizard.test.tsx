// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type {
  ModelInfo,
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
function seedStore(providers: Provider[]) {
  useAppStore.setState({
    providers,
    clients: [],
    backups: [],
    settings: defaultSettings,
    loading: false,
    operation: undefined,
    error: undefined,
    probeResult,
    changes: [],
    applyResults: [],
    probe: mocks.probe,
    saveProvider: mocks.saveProvider,
    reprobeModelReasoning: mocks.reprobeModelReasoning,
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
