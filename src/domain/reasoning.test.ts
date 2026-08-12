import { describe, expect, it } from "vitest";
import type { ReasoningCapability, ReasoningSelection, ReasoningTier, RuntimeVerification, VerificationResult } from "./types";
import {
  advancedOptions,
  appendVerification,
  budgetRange,
  canDisableReasoning,
  canSelectTier,
  hasDynamicBudget,
  latestVerification,
  latestVerificationForTier,
  reasoningUiState,
  tierOptions,
  verifiableTier,
  verificationSummary,
  verificationTierLabel,
  verificationsFor,
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

// —— 运行时验证的纯函数。fixture 刻意用非内置的 effort 取值（xhigh），
// 保证断言检验的是"转发后端结论"而不是某张前端词表。

const supportedCapability: ReasoningCapability = {
  key: { baseUrl: "https://api.example.com/v1", modelId: "test-coder" },
  support: "supported",
  control: { kind: "effortEnum", values: ["minimal", "medium", "xhigh"] },
  tiers: [
    { tier: "light", id: "light", label: "轻度", binding: { kind: "effort", value: "minimal" }, wireSummary: "effort=minimal" },
    { tier: "standard", id: "standard", label: "标准", binding: { kind: "effort", value: "medium" }, wireSummary: "effort=medium" },
    { tier: "deep", id: "deep", label: "深度", binding: { kind: "effort", value: "xhigh" }, wireSummary: "effort=xhigh" },
  ],
  defaultTier: "standard",
  constraints: {},
  confidence: "declared",
  evidence: [],
  discoveredAt: "2026-08-12T10:00:00Z",
  ttlSeconds: 14 * 24 * 3600,
};

function verification(
  overrides: Partial<RuntimeVerification> & { result: VerificationResult; tier: ReasoningTier },
): RuntimeVerification {
  return {
    modelId: "test-coder",
    baseUrl: "https://api.example.com/v1",
    binding: { kind: "effort", value: "medium" },
    verifiedAt: "2026-08-12T10:30:00Z",
    protocol: "openai",
    ...overrides,
  };
}

describe("verificationsFor", () => {
  it("缺 modelId 或缺历史都返回空数组，不抛错", () => {
    const record = verification({ tier: "standard", result: { status: "confirmed" } });
    expect(verificationsFor(undefined, "test-coder")).toEqual([]);
    expect(verificationsFor({ "test-coder": [record] }, undefined)).toEqual([]);
    expect(verificationsFor({ "test-coder": [record] }, "other-model")).toEqual([]);
    expect(verificationsFor({ "test-coder": [record] }, "test-coder")).toEqual([record]);
  });
});

describe("latestVerification", () => {
  /**
   * 取末尾而不是按 verifiedAt 排序。fixture 里"较早"那条用 +08:00 偏移写成 18:00，
   * 真实瞬间是 10:00 UTC，字典序却排在 11:00Z 之后——按字符串排序会挑错。
   */
  it("按追加顺序取最新，不受 RFC3339 偏移量写法影响", () => {
    const earlier = verification({ tier: "light", result: { status: "confirmed" }, verifiedAt: "2026-08-12T18:00:00+08:00" });
    const later = verification({ tier: "deep", result: { status: "confirmed" }, verifiedAt: "2026-08-12T11:00:00Z" });
    // 真实时序 earlier < later，字典序恰好相反：这正是不按字符串排序的理由。
    expect(Date.parse(earlier.verifiedAt) < Date.parse(later.verifiedAt)).toBe(true);
    expect(earlier.verifiedAt > later.verifiedAt).toBe(true);
    expect(latestVerification({ "test-coder": [earlier, later] }, "test-coder")).toBe(later);
  });

  it("空历史返回 undefined", () => {
    expect(latestVerification({}, "test-coder")).toBeUndefined();
    expect(latestVerification({ "test-coder": [] }, "test-coder")).toBeUndefined();
  });
});

describe("latestVerificationForTier", () => {
  it("只看该档位的最后一条，不被其他档位的更晚记录顶掉", () => {
    const deepOk = verification({ tier: "deep", result: { status: "confirmed" } });
    const lightFailed = verification({ tier: "light", result: { status: "failed", error: "超时" } });
    const history = { "test-coder": [deepOk, lightFailed] };
    expect(latestVerificationForTier(history, "test-coder", "deep")).toBe(deepOk);
    expect(latestVerificationForTier(history, "test-coder", "light")).toBe(lightFailed);
    expect(latestVerificationForTier(history, "test-coder", "standard")).toBeUndefined();
    expect(latestVerificationForTier(history, "test-coder", undefined)).toBeUndefined();
  });
});

describe("verifiableTier", () => {
  it("supported 且无显式 binding 时给出生效档位", () => {
    expect(verifiableTier(supportedCapability)).toBe("standard");
    const selection: ReasoningSelection = { modelId: "test-coder", tier: "deep", source: "user", chosenAt: "2026-08-12T10:00:00Z" };
    expect(verifiableTier(supportedCapability, selection)).toBe("deep");
  });

  /** 钉死 binding 后没有可断言的档位，按钮必须能据此禁用。 */
  it("显式 binding 返回 undefined", () => {
    const pinned: ReasoningSelection = {
      modelId: "test-coder",
      explicitBinding: { kind: "effort", value: "xhigh" },
      source: "user",
      chosenAt: "2026-08-12T10:00:00Z",
    };
    expect(verifiableTier(supportedCapability, pinned)).toBeUndefined();
  });

  it("unsupported / unknown / 无能力对象一律返回 undefined", () => {
    expect(verifiableTier({ ...supportedCapability, support: "unsupported" })).toBeUndefined();
    expect(verifiableTier({ ...supportedCapability, support: "unknown" })).toBeUndefined();
    expect(verifiableTier({ ...supportedCapability, tiers: [] })).toBeUndefined();
    expect(verifiableTier(undefined)).toBeUndefined();
  });
});

describe("appendVerification", () => {
  it("追加不覆盖，且不改入参", () => {
    const first = verification({ tier: "standard", result: { status: "confirmed" } });
    const second = verification({ tier: "standard", result: { status: "failed", error: "网络错误" } });
    const start = { "test-coder": [first] };
    const next = appendVerification(start, second);
    expect(next["test-coder"]).toEqual([first, second]);
    expect(start["test-coder"]).toEqual([first]);
  });

  it("空历史与其他模型的历史都能正确处理", () => {
    const record = verification({ tier: "deep", result: { status: "confirmed" } });
    expect(appendVerification(undefined, record)).toEqual({ "test-coder": [record] });
    const other = verification({ tier: "light", result: { status: "confirmed" }, modelId: "other-model" });
    const merged = appendVerification({ "test-coder": [record] }, other);
    expect(merged["test-coder"]).toEqual([record]);
    expect(merged["other-model"]).toEqual([other]);
  });
});

describe("verificationTierLabel", () => {
  it("档位名来自能力表", () => {
    const record = verification({ tier: "deep", result: { status: "confirmed" } });
    expect(verificationTierLabel(record, supportedCapability)).toBe("深度");
  });

  it("能力表里没有这一档时退回后端原始 tier 值，不自造词", () => {
    const record = verification({ tier: "max", result: { status: "confirmed" } });
    expect(verificationTierLabel(record, supportedCapability)).toBe("max");
    expect(verificationTierLabel(record, undefined)).toBe("max");
  });
});

describe("verificationSummary", () => {
  it("confirmed 带档位名，没有 detail", () => {
    const summary = verificationSummary(verification({ tier: "deep", result: { status: "confirmed" } }), supportedCapability);
    expect(summary.status).toBe("confirmed");
    expect(summary.label).toBe("已验证 深度");
    expect(summary.detail).toBeUndefined();
  });

  /** Rejected ≠ Unsupported：文案必须说清是"这次没看到推理产物"，不能说"不支持"。 */
  it("rejected 说明未检测到推理产物，且不含「不支持」", () => {
    const summary = verificationSummary(
      verification({ tier: "standard", result: { status: "rejected", reason: "响应中未检测到 openai 协议的推理字段" } }),
      supportedCapability,
    );
    expect(summary.status).toBe("rejected");
    expect(summary.label).toContain("未检测到推理产物");
    expect(summary.label).toContain("标准");
    expect(summary.detail).toBe("响应中未检测到 openai 协议的推理字段");
    expect(summary.label).not.toContain("不支持");
    expect(summary.detail).not.toContain("不支持");
  });

  /** Failed ≠ Unsupported：请求没走通不构成能力结论。 */
  it("failed 显示验证失败并保留错误原文，且不含「不支持」", () => {
    const summary = verificationSummary(
      verification({ tier: "light", result: { status: "failed", error: "API 错误 429：rate limited" } }),
      supportedCapability,
    );
    expect(summary.status).toBe("failed");
    expect(summary.label).toBe("验证失败");
    expect(summary.detail).toBe("API 错误 429：rate limited");
    expect(summary.label).not.toContain("不支持");
  });

  it("三态文案都不出现「不支持」", () => {
    const records = [
      verification({ tier: "deep", result: { status: "confirmed" } }),
      verification({ tier: "deep", result: { status: "rejected", reason: "无推理字段" } }),
      verification({ tier: "deep", result: { status: "failed", error: "连接被拒绝" } }),
    ];
    for (const record of records) {
      const summary = verificationSummary(record, supportedCapability);
      expect(`${summary.label}${summary.detail ?? ""}`).not.toContain("不支持");
    }
  });
});
