// @vitest-environment jsdom
import { render, screen, cleanup } from "@testing-library/react";
import { describe, expect, it, vi, afterEach } from "vitest";
import type { ReasoningCapability } from "../domain/types";
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
