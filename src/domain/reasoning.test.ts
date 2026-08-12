import { describe, expect, it } from "vitest";
import type { ReasoningCapability } from "./types";
import {
  advancedOptions,
  budgetRange,
  canDisableReasoning,
  canSelectTier,
  hasDynamicBudget,
  reasoningUiState,
  tierOptions,
} from "./reasoning";

describe("reasoningUiState", () => {
  it("动态 tiers 不产生硬编码：输入 xhigh ultra，输出完整包含", () => {
    const capability: ReasoningCapability = {
      key: { baseUrl: "https://api.example.com", modelId: "model-x" },
      support: "supported",
      control: { kind: "effortEnum", values: ["minimal", "medium", "xhigh", "ultra"] },
      tiers: [
        { tier: "light", id: "light", label: "轻度", binding: { kind: "effort", value: "minimal" }, wireSummary: "effort=minimal" },
        { tier: "standard", id: "standard", label: "标准", binding: { kind: "effort", value: "medium" }, wireSummary: "effort=medium" },
        { tier: "deep", id: "deep", label: "深度", binding: { kind: "effort", value: "xhigh" }, wireSummary: "effort=xhigh" },
        { tier: "max", id: "max", label: "极限", binding: { kind: "effort", value: "ultra" }, wireSummary: "effort=ultra" },
      ],
      defaultTier: "standard",
      constraints: {},
      confidence: "declared",
      evidence: [],
      discoveredAt: new Date().toISOString(),
      ttlSeconds: 14 * 24 * 3600,
    };
    expect(reasoningUiState(capability)).toBe("supported");
    const options = tierOptions(capability);
    expect(options).toHaveLength(4);
    const labels = options.map((option) => option.label);
    expect(labels).toEqual(["轻度", "标准", "深度", "极限"]);
    const bindings = options.map((option) => option.binding);
    expect(bindings).toContainEqual({ kind: "effort", value: "xhigh" });
    expect(bindings).toContainEqual({ kind: "effort", value: "ultra" });
    const advanced = advancedOptions(capability);
    expect(advanced).toEqual(["minimal", "medium", "xhigh", "ultra"]);
  });

  it("unknown: uiState === unknown, tiers 为空", () => {
    const capability: ReasoningCapability = {
      key: { baseUrl: "https://api.example.com", modelId: "model-unknown" },
      support: "unknown",
      control: { kind: "none" },
      tiers: [],
      constraints: {},
      confidence: "unknown",
      evidence: [],
      discoveredAt: new Date().toISOString(),
      ttlSeconds: 6 * 3600,
    };
    expect(reasoningUiState(capability)).toBe("unknown");
    expect(canSelectTier(capability)).toBe(false);
    expect(tierOptions(capability)).toEqual([]);
  });

  it("unsupported: uiState === unsupported, 没有档位", () => {
    const capability: ReasoningCapability = {
      key: { baseUrl: "https://api.example.com", modelId: "model-basic" },
      support: "unsupported",
      control: { kind: "none" },
      tiers: [],
      constraints: {},
      confidence: "validated",
      evidence: [{ source: "introspection", detail: "模型列表未声明推理能力", observedAt: new Date().toISOString() }],
      discoveredAt: new Date().toISOString(),
      ttlSeconds: 24 * 3600,
    };
    expect(reasoningUiState(capability)).toBe("unsupported");
    expect(canSelectTier(capability)).toBe(false);
    expect(tierOptions(capability)).toEqual([]);
  });

  it("budget: dynamicSentinel 存在", () => {
    const capability: ReasoningCapability = {
      key: { baseUrl: "https://api.example.com", modelId: "model-budget" },
      support: "supported",
      control: { kind: "tokenBudget", min: 1024, max: 16384, offAllowed: true, dynamicSentinel: -1 },
      tiers: [
        { tier: "light", id: "light", label: "轻度", binding: { kind: "budget", tokens: 2048 }, wireSummary: "budget=2048" },
        { tier: "standard", id: "standard", label: "自动", binding: { kind: "dynamicBudget", sentinel: -1 }, wireSummary: "budget=-1" },
      ],
      constraints: {},
      confidence: "validated",
      evidence: [],
      discoveredAt: new Date().toISOString(),
      ttlSeconds: 14 * 24 * 3600,
    };
    expect(reasoningUiState(capability)).toBe("supported");
    const range = budgetRange(capability);
    expect(range).toBeDefined();
    expect(range?.min).toBe(1024);
    expect(range?.max).toBe(16384);
    expect(range?.dynamicSentinel).toBe(-1);
    expect(hasDynamicBudget(capability)).toBe(true);
  });

  it("boolean: cannotDisable 时无关闭选项", () => {
    const capability: ReasoningCapability = {
      key: { baseUrl: "https://api.example.com", modelId: "model-toggle" },
      support: "supported",
      control: { kind: "booleanToggle" },
      tiers: [{ tier: "standard", id: "standard", label: "启用推理", binding: { kind: "enabled" }, wireSummary: "enabled" }],
      constraints: { cannotDisable: true },
      confidence: "declared",
      evidence: [],
      discoveredAt: new Date().toISOString(),
      ttlSeconds: 14 * 24 * 3600,
    };
    expect(reasoningUiState(capability)).toBe("supported");
    expect(canDisableReasoning(capability)).toBe(false);
    expect(tierOptions(capability).some((option) => option.tier === "off")).toBe(false);
  });
});
