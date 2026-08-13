// @vitest-environment jsdom
import { render, screen, cleanup, fireEvent } from "@testing-library/react";
import { describe, expect, it, vi, afterEach } from "vitest";
import type {
  ReasoningCapability,
  ReasoningSelection,
  ReasoningTier,
  RuntimeVerification,
  VerificationResult,
} from "../domain/types";
import type { FallbackSettings } from "../domain/reasoning";
import { fallbackNotice } from "../domain/reasoning";
import { ReasoningTierPicker } from "./ReasoningTierPicker";

describe("ReasoningTierPicker", () => {
  afterEach(() => {
    cleanup();
  });
  it("effort 显示真实 tier", () => {
    const capability: ReasoningCapability = {
      key: { baseUrl: "https://api.example.com", modelId: "test-model" },
      support: "supported",
      control: { kind: "effortEnum", values: ["minimal", "medium", "xhigh"] },
      tiers: [
        { tier: "light", id: "light", label: "轻度推理", binding: { kind: "effort", value: "minimal" }, wireSummary: "reasoning.effort = minimal" },
        { tier: "standard", id: "standard", label: "标准推理", binding: { kind: "effort", value: "medium" }, wireSummary: "reasoning.effort = medium" },
        { tier: "deep", id: "deep", label: "深度推理", binding: { kind: "effort", value: "xhigh" }, wireSummary: "reasoning.effort = xhigh" },
      ],
      defaultTier: "standard",
      constraints: {},
      confidence: "validated",
      evidence: [],
      discoveredAt: new Date().toISOString(),
      ttlSeconds: 14 * 24 * 3600,
    };
    render(<ReasoningTierPicker capability={capability} onChange={vi.fn()} />);
    expect(screen.getByText("轻度推理")).toBeInTheDocument();
    expect(screen.getByText("标准推理")).toBeInTheDocument();
    expect(screen.getByText("深度推理")).toBeInTheDocument();
    expect(screen.getByText("reasoning.effort = minimal")).toBeInTheDocument();
    expect(screen.getByText("reasoning.effort = xhigh")).toBeInTheDocument();
  });

  it("budget 显示预算", () => {
    const capability: ReasoningCapability = {
      key: { baseUrl: "https://api.example.com", modelId: "budget-model" },
      support: "supported",
      control: { kind: "tokenBudget", min: 1024, max: 24576, offAllowed: true, dynamicSentinel: -1 },
      tiers: [
        { tier: "light", id: "light", label: "轻度", binding: { kind: "budget", tokens: 2048 }, wireSummary: "预算 2048 tokens" },
        { tier: "standard", id: "standard", label: "中度", binding: { kind: "dynamicBudget", sentinel: -1 }, wireSummary: "预算 -1（自动分配）" },
      ],
      constraints: {},
      confidence: "validated",
      evidence: [],
      discoveredAt: new Date().toISOString(),
      ttlSeconds: 14 * 24 * 3600,
    };
    render(<ReasoningTierPicker capability={capability} onChange={vi.fn()} />);
    expect(screen.getByText(/可调预算范围/)).toBeInTheDocument();
    expect(screen.getByText(/1,024.*24,576/)).toBeInTheDocument();
    expect(screen.getByText(/模型自行分配/)).toBeInTheDocument();
  });

  it("boolean 显示 Switch", () => {
    const capability: ReasoningCapability = {
      key: { baseUrl: "https://api.example.com", modelId: "toggle-model" },
      support: "supported",
      control: { kind: "booleanToggle" },
      tiers: [{ tier: "standard", id: "standard", label: "启用推理", binding: { kind: "enabled" }, wireSummary: "thinking enabled" }],
      constraints: {},
      confidence: "declared",
      evidence: [],
      discoveredAt: new Date().toISOString(),
      ttlSeconds: 14 * 24 * 3600,
    };
    render(<ReasoningTierPicker capability={capability} onChange={vi.fn()} />);
    expect(screen.getByRole("switch", { name: /启用推理/ })).toBeInTheDocument();
    expect(screen.getByText("thinking enabled")).toBeInTheDocument();
  });

  it("unknown: 存在重新探测按钮，不存在「轻度 中度 高」", () => {
    const capability: ReasoningCapability = {
      key: { baseUrl: "https://api.example.com", modelId: "unknown-model" },
      support: "unknown",
      control: { kind: "none" },
      tiers: [],
      constraints: {},
      confidence: "unknown",
      evidence: [],
      discoveredAt: new Date().toISOString(),
      ttlSeconds: 6 * 3600,
    };
    const onReprobe = vi.fn();
    render(<ReasoningTierPicker capability={capability} onChange={vi.fn()} onReprobe={onReprobe} />);
    expect(screen.getByText("能力未探明")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /重新探测/ })).toBeInTheDocument();
    expect(screen.queryByText("轻度")).not.toBeInTheDocument();
    expect(screen.queryByText("中度")).not.toBeInTheDocument();
    expect(screen.queryByText("高")).not.toBeInTheDocument();
  });
});

// —— 兜底档位的展示。
//
// 这一组的共同前提：`fallback` 是第三个独立入参，既不来自 capability 也不来自
// verifications。断言的重点不是"显示了什么值"，而是"有没有把用户设定说成探测结论"。

describe("ReasoningTierPicker 的兜底档位标记", () => {
  afterEach(cleanup);
  const unknownCapability: ReasoningCapability = {
    key: { baseUrl: "https://api.example.com/v1", modelId: "test-coder" },
    support: "unknown",
    control: { kind: "none" },
    tiers: [],
    constraints: {},
    confidence: "unknown",
    evidence: [],
    discoveredAt: "2026-08-12T10:00:00Z",
    ttlSeconds: 6 * 3600,
  };
  const settings: FallbackSettings = {
    effectiveReasoningLevel: "medium",
    reasoningFallbacks: [{ modelId: "test-coder", tierId: "deep" }],
  };

  it("未探明时显示兜底档位，并注明是用户设定", () => {
    render(<ReasoningTierPicker capability={unknownCapability} onChange={vi.fn()} fallback={fallbackNotice(unknownCapability, settings, "test-coder")} />);
    expect(screen.getByText("能力未探明")).toBeInTheDocument();
    expect(screen.getByText("高")).toBeInTheDocument();
    expect(screen.getByText(/单模型兜底档位：高（仅配置生效）/)).toBeInTheDocument();
    expect(screen.getByText(/仅用于写入配置文件/)).toBeInTheDocument();
  });

  it("自定义档位额外打「自定义」标记，内置档位不打", () => {
    const custom: FallbackSettings = {
      effectiveReasoningLevel: "medium",
      customReasoningTiers: [{ id: "tier-x", label: "超深", openaiParams: { reasoning: { effort: "xhigh" } } }],
      reasoningNameRules: [{ id: "r", pattern: "test-", matchType: "prefix", tierId: "tier-x" }],
    };
    render(<ReasoningTierPicker capability={unknownCapability} onChange={vi.fn()} fallback={fallbackNotice(unknownCapability, custom, "test-coder")} />);
    expect(screen.getByText("超深")).toBeInTheDocument();
    expect(screen.getByText(/自定义/)).toBeInTheDocument();
    expect(screen.getByText(/名称规则兜底：超深（仅配置生效）/)).toBeInTheDocument();

    cleanup();
    render(<ReasoningTierPicker capability={unknownCapability} onChange={vi.fn()} fallback={fallbackNotice(unknownCapability, settings, "test-coder")} />);
    expect(screen.queryByText(/自定义/)).not.toBeInTheDocument();
  });

  it("兜底文案不出现任何事实性措辞", () => {
    const notice = fallbackNotice(unknownCapability, settings, "test-coder");
    // 「支持/兼容/已确认」会让用户以为自己填的档位得到了服务端确认。
    for (const word of ["支持", "兼容", "已确认"]) {
      expect(notice?.message).not.toContain(word);
    }
    expect(notice?.message).toContain("仅配置生效");
  });

  it("兜底标记不出现 confidence 用词", () => {
    const notice = fallbackNotice(unknownCapability, settings, "test-coder");
    render(<ReasoningTierPicker capability={unknownCapability} onChange={vi.fn()} fallback={notice} />);
    const note = screen.getByText(/单模型兜底档位/).closest("p");
    for (const word of ["服务端声明", "参数校验确认", "真实响应证实"]) {
      expect(note?.textContent).not.toContain(word);
    }
    // 徽章本身不能自称已探明。整段文字里"探测成功后自动改用已探明档位"是对未来的
    // 说明，不是对当前状态的断言，所以只对 label 做这条断言。
    expect(notice?.label).not.toContain("已探明");
  });

  it("已探明不支持：不渲染任何兜底提示", () => {
    const unsupported: ReasoningCapability = { ...unknownCapability, support: "unsupported" };
    render(<ReasoningTierPicker capability={unsupported} onChange={vi.fn()} fallback={fallbackNotice(unsupported, settings, "test-coder")} />);
    expect(screen.getByText("此模型不支持推理")).toBeInTheDocument();
    expect(screen.queryByText(/兜底档位/)).not.toBeInTheDocument();
    expect(screen.queryByText("高")).not.toBeInTheDocument();
  });

  it("没有兜底入参时不多渲染一行", () => {
    render(<ReasoningTierPicker capability={unknownCapability} onChange={vi.fn()} />);
    expect(screen.queryByText(/兜底档位/)).not.toBeInTheDocument();
  });
});

// —— 运行时验证的展示。
//
// 这一组测试的共同前提：`capability` 与 `verifications` 是两个独立入参。
// 断言里刻意不出现 confidence 的任何变化——它由 capability 单方面决定，
// 验证结果无论三态如何都不该影响它。

const verifiableCapability: ReasoningCapability = {
  key: { baseUrl: "https://api.example.com/v1", modelId: "test-coder" },
  support: "supported",
  control: { kind: "effortEnum", values: ["minimal", "medium", "xhigh"] },
  tiers: [
    { tier: "light", id: "light", label: "轻度推理", binding: { kind: "effort", value: "minimal" }, wireSummary: "reasoning.effort = minimal" },
    { tier: "standard", id: "standard", label: "标准推理", binding: { kind: "effort", value: "medium" }, wireSummary: "reasoning.effort = medium" },
    { tier: "deep", id: "deep", label: "深度推理", binding: { kind: "effort", value: "xhigh" }, wireSummary: "reasoning.effort = xhigh" },
  ],
  defaultTier: "standard",
  constraints: {},
  confidence: "declared",
  evidence: [],
  discoveredAt: "2026-08-12T10:00:00Z",
  ttlSeconds: 14 * 24 * 3600,
};

function record(tier: ReasoningTier, result: VerificationResult, verifiedAt = "2026-08-12T10:30:00Z"): RuntimeVerification {
  return {
    modelId: "test-coder",
    baseUrl: "https://api.example.com/v1",
    tier,
    binding: { kind: "effort", value: "medium" },
    result,
    verifiedAt,
    protocol: "openai",
  };
}

const historyOf = (...records: RuntimeVerification[]) => ({ "test-coder": records });

describe("ReasoningTierPicker 的运行时验证", () => {
  afterEach(() => cleanup());

  it("点击验证按钮时按当前生效档位调用 onVerify", async () => {
    const onVerify = vi.fn();
    render(<ReasoningTierPicker capability={verifiableCapability} onChange={vi.fn()} onVerify={onVerify} />);

    const button = screen.getByRole("button", { name: /验证「标准推理」档位/ });
    fireEvent.click(button);

    expect(onVerify).toHaveBeenCalledTimes(1);
    // 生效档位是 capability.defaultTier，不是列表里的第一档。
    expect(onVerify).toHaveBeenCalledWith("standard");
  });

  it("用户选择过档位时按所选档位验证", () => {
    const onVerify = vi.fn();
    const selection: ReasoningSelection = { modelId: "test-coder", tier: "deep", source: "user", chosenAt: "2026-08-12T10:00:00Z" };
    render(<ReasoningTierPicker capability={verifiableCapability} selection={selection} onChange={vi.fn()} onVerify={onVerify} />);

    fireEvent.click(screen.getByRole("button", { name: /验证「深度推理」档位/ }));

    expect(onVerify).toHaveBeenCalledWith("deep");
  });

  it("按钮附近提示会发出真实请求并可能产生费用", () => {
    render(<ReasoningTierPicker capability={verifiableCapability} onChange={vi.fn()} onVerify={vi.fn()} />);
    expect(screen.getByText(/会向该端点发送一次真实请求，可能产生 API 使用费用/)).toBeInTheDocument();
  });

  it("confirmed 显示已验证与档位名，且不显示为官方支持或置信度提升", () => {
    render(
      <ReasoningTierPicker
        capability={verifiableCapability}
        onChange={vi.fn()}
        onVerify={vi.fn()}
        verifications={historyOf(record("standard", { status: "confirmed" }))}
      />,
    );

    expect(screen.getByText("已验证 标准推理")).toBeInTheDocument();
    // header 里的置信度仍是能力表的 declared，验证没有把它抬成"真实响应证实"。
    expect(screen.getByText("服务端声明")).toBeInTheDocument();
    expect(screen.queryByText("真实响应证实")).not.toBeInTheDocument();
    expect(screen.queryByText(/官方支持/)).not.toBeInTheDocument();
  });

  it("rejected 显示未检测到推理产物、保留 reason，且不出现「不支持」", () => {
    render(
      <ReasoningTierPicker
        capability={verifiableCapability}
        onChange={vi.fn()}
        onVerify={vi.fn()}
        verifications={historyOf(record("standard", { status: "rejected", reason: "响应中未检测到 openai 协议的推理字段" }))}
      />,
    );

    expect(screen.getByText(/此 endpoint 下「标准推理」未检测到推理产物/)).toBeInTheDocument();
    expect(screen.getByText(/响应中未检测到 openai 协议的推理字段/)).toBeInTheDocument();
    // Rejected ≠ Unsupported：整块渲染结果里都不许出现"不支持"。
    expect(document.body.textContent).not.toContain("不支持");
  });

  it("failed 显示验证失败、保留 error，且不出现「不支持」", () => {
    render(
      <ReasoningTierPicker
        capability={verifiableCapability}
        onChange={vi.fn()}
        onVerify={vi.fn()}
        verifications={historyOf(record("standard", { status: "failed", error: "API 错误 429：rate limited" }))}
      />,
    );

    expect(screen.getByText("验证失败")).toBeInTheDocument();
    expect(screen.getByText(/API 错误 429：rate limited/)).toBeInTheDocument();
    expect(document.body.textContent).not.toContain("不支持");
  });

  it("多条历史可展开，三条文案都不出现「不支持」", () => {
    render(
      <ReasoningTierPicker
        capability={verifiableCapability}
        onChange={vi.fn()}
        onVerify={vi.fn()}
        verifications={historyOf(
          record("deep", { status: "confirmed" }, "2026-08-12T10:00:00Z"),
          record("standard", { status: "rejected", reason: "无推理字段" }, "2026-08-12T10:20:00Z"),
          record("standard", { status: "failed", error: "连接被拒绝" }, "2026-08-12T10:40:00Z"),
        )}
      />,
    );

    expect(screen.getByText(/验证历史（3 条）/)).toBeInTheDocument();
    // 顶部徽章看的是当前档位（standard）的最后一条，即 failed，不是全局最后一条。
    expect(screen.getAllByText("验证失败").length).toBeGreaterThan(0);
    expect(screen.getByText("已验证 深度推理")).toBeInTheDocument();
    expect(document.body.textContent).not.toContain("不支持");
  });

  it("verifying 时按钮禁用并显示进行中", () => {
    const onVerify = vi.fn();
    render(<ReasoningTierPicker capability={verifiableCapability} onChange={vi.fn()} onVerify={onVerify} verifying />);

    const button = screen.getByRole("button", { name: /正在验证/ });
    expect(button).toBeDisabled();
    fireEvent.click(button);
    expect(onVerify).not.toHaveBeenCalled();
  });

  it("unsupported 与 unknown 都不显示验证入口", () => {
    render(
      <ReasoningTierPicker
        capability={{ ...verifiableCapability, support: "unsupported", tiers: [] }}
        onChange={vi.fn()}
        onVerify={vi.fn()}
      />,
    );
    expect(screen.queryByRole("button", { name: /验证/ })).not.toBeInTheDocument();
    cleanup();

    render(
      <ReasoningTierPicker
        capability={{ ...verifiableCapability, support: "unknown", tiers: [] }}
        onChange={vi.fn()}
        onVerify={vi.fn()}
      />,
    );
    expect(screen.queryByRole("button", { name: /验证/ })).not.toBeInTheDocument();
  });

  it("explicitBinding 钉死取值时按钮禁用并说明原因", () => {
    const onVerify = vi.fn();
    const pinned: ReasoningSelection = {
      modelId: "test-coder",
      explicitBinding: { kind: "effort", value: "xhigh" },
      source: "user",
      chosenAt: "2026-08-12T10:00:00Z",
    };
    render(<ReasoningTierPicker capability={verifiableCapability} selection={pinned} onChange={vi.fn()} onVerify={onVerify} />);

    const button = screen.getByRole("button", { name: /验证当前档位/ });
    expect(button).toBeDisabled();
    fireEvent.click(button);
    expect(onVerify).not.toHaveBeenCalled();
    expect(screen.getByText(/没有可断言的语义档位/)).toBeInTheDocument();
  });

  it("未传 onVerify 时整块验证区不渲染，即使已有历史", () => {
    render(
      <ReasoningTierPicker
        capability={verifiableCapability}
        onChange={vi.fn()}
        verifications={historyOf(record("standard", { status: "confirmed" }))}
      />,
    );

    expect(screen.queryByRole("button", { name: /验证/ })).not.toBeInTheDocument();
    expect(screen.queryByText("已验证 标准推理")).not.toBeInTheDocument();
    expect(screen.queryByLabelText("运行时验证")).not.toBeInTheDocument();
  });

  it("没有历史时显示尚未验证，按钮仍可用", () => {
    render(<ReasoningTierPicker capability={verifiableCapability} onChange={vi.fn()} onVerify={vi.fn()} />);
    expect(screen.getByText("尚未验证")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /验证「标准推理」档位/ })).toBeEnabled();
  });
});
