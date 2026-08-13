import { describe, expect, it } from "vitest";
import type {
  CustomReasoningTier,
  MatchedCustomTier,
  ModelReasoningMeta,
  ReasoningCapability,
  ReasoningFallback,
  ReasoningLevel,
  ReasoningNameRule,
  ReasoningSelection,
  ReasoningTier,
  RuntimeVerification,
  VerificationResult,
} from "./types";
import type { FallbackSettings } from "./reasoning";
import {
  advancedOptions,
  appendVerification,
  budgetRange,
  builtinReasoningTiers,
  canDisableReasoning,
  canSelectTier,
  effectiveFallbackTier,
  fallbackFor,
  fallbackNotice,
  hasDynamicBudget,
  latestVerification,
  latestVerificationForTier,
  makeSelection,
  matchNameRule,
  originLabel,
  reasoningOrigin,
  reasoningUiState,
  removeFallback,
  resolveTierLabel,
  tierOptions,
  tierPickerGroups,
  upsertFallback,
  verifiableTier,
  verificationSummary,
  verificationTierLabel,
  verificationsFor,
  writeTargetSummary,
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

describe("兜底档位与已探明档位的区分", () => {
  const settings = (fallbacks: ReasoningFallback[], level: ReasoningLevel = "medium"): FallbackSettings =>
    ({ effectiveReasoningLevel: level, reasoningFallbacks: fallbacks });
  const unknownCapability: ReasoningCapability = { ...supportedCapability, support: "unknown", tiers: [], confidence: "unknown" };
  const unsupportedCapability: ReasoningCapability = { ...supportedCapability, support: "unsupported", tiers: [] };

  it("能力未探明且有逐模型设定：用该设定，来源标成用户设定", () => {
    const state = settings([{ modelId: "test-coder", tierId: "deep" }]);
    expect(reasoningOrigin(unknownCapability, state, "test-coder")).toBe("model-fallback");
    expect(effectiveFallbackTier(unknownCapability, state, "test-coder")).toEqual({ label: "高", custom: false });
    expect(originLabel("model-fallback")).toContain("用户");
  });

  it("能力未探明且没有逐模型设定：退到全局回退档", () => {
    const state = settings([], "low");
    expect(reasoningOrigin(unknownCapability, state, "test-coder")).toBe("global-fallback");
    expect(effectiveFallbackTier(unknownCapability, state, "test-coder")).toEqual({ label: "low", custom: false });
  });

  it("能力缺失（从未探测）同样走兜底", () => {
    const state = settings([{ modelId: "test-coder", tierId: "deep" }]);
    expect(reasoningOrigin(undefined, state, "test-coder")).toBe("model-fallback");
    expect(reasoningOrigin(null, settings([]), "test-coder")).toBe("global-fallback");
  });

  it("已探明支持：兜底完全不参与，即便用户为这个模型设了值", () => {
    const state = settings([{ modelId: "test-coder", tierId: "deep" }]);
    expect(reasoningOrigin(supportedCapability, state, "test-coder")).toBe("discovered");
    expect(effectiveFallbackTier(supportedCapability, state, "test-coder")).toBeUndefined();
    expect(fallbackNotice(supportedCapability, state, "test-coder")).toBeUndefined();
  });

  it("已探明不支持：用户兜底不覆盖这个事实", () => {
    const state = settings([{ modelId: "test-coder", tierId: "deep" }]);
    expect(reasoningOrigin(unsupportedCapability, state, "test-coder")).toBe("omitted");
    expect(effectiveFallbackTier(unsupportedCapability, state, "test-coder")).toBeUndefined();
    expect(fallbackNotice(unsupportedCapability, state, "test-coder")).toBeUndefined();
  });

  it("声明支持却没有可用档位：也不套兜底，与后端一致地省略", () => {
    const empty: ReasoningCapability = { ...supportedCapability, tiers: [], defaultTier: undefined };
    expect(reasoningUiState(empty)).toBe("empty");
    expect(reasoningOrigin(empty, settings([{ modelId: "test-coder", tierId: "deep" }]), "test-coder")).toBe("omitted");
  });

  it("modelId 全等匹配，前后空格和大小写都不命中", () => {
    const fallbacks: ReasoningFallback[] = [{ modelId: "test-coder", tierId: "deep" }];
    expect(fallbackFor(fallbacks, "test-coder")).toBe("deep");
    for (const candidate of ["test-coder-v2", "test-code", "Test-Coder", "TEST-CODER", " test-coder", "test-coder "]) {
      expect(fallbackFor(fallbacks, candidate)).toBeUndefined();
    }
    expect(fallbackFor(fallbacks, undefined)).toBeUndefined();
    expect(fallbackFor(undefined, "test-coder")).toBeUndefined();
  });

  it("兜底措辞不复用 confidence 的任何用词", () => {
    const notice = fallbackNotice(unknownCapability, settings([]), "test-coder");
    expect(notice).toBeDefined();
    for (const word of ["声明", "校验", "证实", "已探明"]) {
      expect(notice?.label).not.toContain(word);
    }
    expect(notice?.label).toContain("兜底");
    expect(notice?.tier).toBe("medium");
    // 「支持/兼容/已确认」是事实性措辞，兜底文案里一个都不许出现。
    for (const word of ["支持", "兼容", "已确认"]) {
      expect(notice?.message).not.toContain(word);
    }
    expect(notice?.message).toContain("仅配置生效");
  });

  it("upsert 同一模型只留一条，并 trim modelId", () => {
    let list = upsertFallback([], { modelId: " test-coder ", tierId: "light" });
    expect(list).toEqual([{ modelId: "test-coder", tierId: "light" }]);
    list = upsertFallback(list, { modelId: "test-coder", tierId: "deep" });
    expect(list).toEqual([{ modelId: "test-coder", tierId: "deep" }]);
    list = upsertFallback(list, { modelId: "other", tierId: "standard" });
    expect(list).toHaveLength(2);
  });

  it("upsert 忽略空 modelId，remove 只删指定模型", () => {
    const list: ReasoningFallback[] = [{ modelId: "a", tierId: "light" }, { modelId: "b", tierId: "deep" }];
    expect(upsertFallback(list, { modelId: "   ", tierId: "deep" })).toBe(list);
    expect(removeFallback(list, "a")).toEqual([{ modelId: "b", tierId: "deep" }]);
    expect(removeFallback(list, "missing")).toEqual(list);
    expect(removeFallback(undefined, "a")).toEqual([]);
  });

  it("探测成功后自动改用已探明档位，无需清理兜底表", () => {
    const state = settings([{ modelId: "test-coder", tierId: "deep" }]);
    expect(reasoningOrigin(unknownCapability, state, "test-coder")).toBe("model-fallback");
    // 同一张兜底表，能力一变成 supported 就不再生效——这就是"自动升级"。
    expect(reasoningOrigin(supportedCapability, state, "test-coder")).toBe("discovered");
  });
});

// —— 自定义档位与模型名规则。
//
// 这一组的共同前提：三张表全部由用户维护，默认为空。断言分两类——
// 一类钉住"空表时行为与旧版一致"，一类钉住"任何失效都降级而不是报错"。

describe("自定义档位与模型名规则", () => {
  const unknownCapability: ReasoningCapability = { ...supportedCapability, support: "unknown", tiers: [], confidence: "unknown" };
  const customTier: CustomReasoningTier = {
    id: "tier-x",
    label: "超深",
    openaiParams: { reasoning: { effort: "xhigh" } },
  };
  const base = (extra: Partial<FallbackSettings> = {}): FallbackSettings =>
    ({ effectiveReasoningLevel: "medium", reasoningFallbacks: [], customReasoningTiers: [], reasoningNameRules: [], ...extra });

  it("三张表全空时退到全局回退档，与旧版本行为一致", () => {
    const state = base();
    expect(reasoningOrigin(unknownCapability, state, "glm-4-plus")).toBe("global-fallback");
    expect(effectiveFallbackTier(unknownCapability, state, "glm-4-plus")).toEqual({ label: "medium", custom: false });
  });

  it("内置档位 id 解析成中文名，自定义档位标出 custom", () => {
    expect(resolveTierLabel("light", [])).toEqual({ label: "轻度", custom: false });
    expect(resolveTierLabel("tier-x", [customTier])).toEqual({ label: "超深", custom: true });
    expect(resolveTierLabel("tier-x", [])).toBeUndefined();
    expect(resolveTierLabel(undefined, [customTier])).toBeUndefined();
    // 名字留空时退回 id，不显示成空白。
    expect(resolveTierLabel("t", [{ id: "t", label: "  " }])).toEqual({ label: "t", custom: true });
  });

  it("前缀与包含两种匹配都大小写不敏感", () => {
    const rules: ReasoningNameRule[] = [
      { id: "r1", pattern: "GLM-", matchType: "prefix", tierId: "light" },
      { id: "r2", pattern: "THINKING", matchType: "contains", tierId: "deep" },
    ];
    expect(matchNameRule(rules, "glm-4-plus")?.id).toBe("r1");
    expect(matchNameRule(rules, "qwen-thinking-max")?.id).toBe("r2");
    // 前缀规则不该被"名字中间含有该片段"命中。
    expect(matchNameRule(rules, "custom-glm-4")?.id).toBe(undefined);
    expect(matchNameRule(rules, "gpt-4o")).toBeUndefined();
    expect(matchNameRule(rules, undefined)).toBeUndefined();
    expect(matchNameRule(undefined, "glm-4")).toBeUndefined();
  });

  it("多条规则按数组顺序取首个命中", () => {
    const rules: ReasoningNameRule[] = [
      { id: "first", pattern: "glm-", matchType: "prefix", tierId: "light" },
      { id: "second", pattern: "glm-4", matchType: "prefix", tierId: "deep" },
    ];
    expect(matchNameRule(rules, "glm-4-plus")?.id).toBe("first");
    // 顺序颠倒，答案跟着变——顺序就是用户表达的优先级。
    expect(matchNameRule([rules[1], rules[0]], "glm-4-plus")?.id).toBe("second");
  });

  it("空 pattern 不参与匹配：否则它会命中一切模型", () => {
    const rules: ReasoningNameRule[] = [{ id: "blank", pattern: "   ", matchType: "contains", tierId: "deep" }];
    expect(matchNameRule(rules, "anything")).toBeUndefined();
  });

  it("命中名称规则时来源是 name-rule，文案用「名称规则兜底」", () => {
    const state = base({
      customReasoningTiers: [customTier],
      reasoningNameRules: [{ id: "r", pattern: "glm-", matchType: "prefix", tierId: "tier-x" }],
    });
    expect(reasoningOrigin(unknownCapability, state, "glm-4-plus")).toBe("name-rule");
    const notice = fallbackNotice(unknownCapability, state, "glm-4-plus");
    expect(notice?.message).toBe("名称规则兜底：超深（仅配置生效）");
    expect(notice?.custom).toBe(true);
  });

  it("单模型兜底压过名称规则：具体意图胜过宽泛意图", () => {
    const state = base({
      reasoningFallbacks: [{ modelId: "glm-4-plus", tierId: "light" }],
      reasoningNameRules: [{ id: "r", pattern: "glm-", matchType: "prefix", tierId: "deep" }],
    });
    expect(reasoningOrigin(unknownCapability, state, "glm-4-plus")).toBe("model-fallback");
    expect(fallbackNotice(unknownCapability, state, "glm-4-plus")?.message).toBe("单模型兜底档位：轻度（仅配置生效）");
    // 同一份配置，另一个模型仍然走规则。
    expect(reasoningOrigin(unknownCapability, state, "glm-3-turbo")).toBe("name-rule");
  });

  it("单模型兜底指向已删除的档位时降级到名称规则", () => {
    const state = base({
      reasoningFallbacks: [{ modelId: "glm-4-plus", tierId: "deleted" }],
      reasoningNameRules: [{ id: "r", pattern: "glm-", matchType: "prefix", tierId: "deep" }],
    });
    expect(reasoningOrigin(unknownCapability, state, "glm-4-plus")).toBe("name-rule");
    expect(effectiveFallbackTier(unknownCapability, state, "glm-4-plus")).toEqual({ label: "高", custom: false });
  });

  it("两级都指向已删除的档位时降级到全局回退档，不报错", () => {
    const state = base({
      reasoningFallbacks: [{ modelId: "glm-4-plus", tierId: "gone" }],
      reasoningNameRules: [{ id: "r", pattern: "glm-", matchType: "prefix", tierId: "also-gone" }],
    });
    expect(reasoningOrigin(unknownCapability, state, "glm-4-plus")).toBe("global-fallback");
    expect(fallbackNotice(unknownCapability, state, "glm-4-plus")?.message).toBe("全局回退档：medium（仅配置生效）");
  });

  it("已探明支持时，自定义档位和名称规则一律不参与", () => {
    const state = base({
      customReasoningTiers: [customTier],
      reasoningNameRules: [{ id: "r", pattern: "test-", matchType: "prefix", tierId: "tier-x" }],
    });
    expect(reasoningOrigin(supportedCapability, state, "test-coder")).toBe("discovered");
    expect(fallbackNotice(supportedCapability, state, "test-coder")).toBeUndefined();
  });

  it("内置档位清单是封闭的五档，顺序固定", () => {
    expect(builtinReasoningTiers.map((tier) => tier.id)).toEqual(["off", "light", "standard", "deep", "max"]);
    expect(builtinReasoningTiers.map((tier) => tier.label)).toEqual(["关闭", "轻度", "中度", "高", "最大"]);
  });
});

// —— 档位下拉的分组。
//
// 这一组钉住三件事：段序固定、空段不产出、`meta` 缺失不退化成"不支持"。
// 所有 label 必须来自入参，任何一条断言里都不该出现本模块自造的档位名。

describe("tierPickerGroups", () => {
  const unknownCapability: ReasoningCapability = { ...supportedCapability, support: "unknown", tiers: [], confidence: "unknown" };
  const meta = (extra: Partial<ModelReasoningMeta> = {}): ModelReasoningMeta => ({
    supportedProtocols: ["openai"],
    nativeParamKind: "effort-enum",
    matchedCustomTiers: [],
    ...extra,
  });
  const matched: MatchedCustomTier[] = [
    { tierId: "tier-x", label: "超深", rulePattern: "test-", ruleMatchType: "prefix", supportedProtocols: ["openai", "azure-openai", "custom"] },
    { tierId: "tier-y", label: "极限", rulePattern: "coder", ruleMatchType: "contains", supportedProtocols: ["anthropic"] },
  ];
  const state: FallbackSettings = { effectiveReasoningLevel: "medium" };

  it("三段顺序固定，且匹配段保留用户规则表的顺序", () => {
    const groups = tierPickerGroups(unknownCapability, meta({ matchedCustomTiers: matched }), state);
    expect(groups.map((group) => group.kind)).toEqual(["matched-custom", "builtin", "global-fallback"]);
    expect(groups[0].items.map((item) => item.id)).toEqual(["tier-x", "tier-y"]);
    // 顺序颠倒，输出跟着变——顺序是用户表达的优先级，本函数不重排。
    const reversed = tierPickerGroups(unknownCapability, meta({ matchedCustomTiers: [matched[1], matched[0]] }), state);
    expect(reversed[0].items.map((item) => item.id)).toEqual(["tier-y", "tier-x"]);
  });

  it("匹配段带命中说明，措辞取自规则本身", () => {
    const groups = tierPickerGroups(unknownCapability, meta({ matchedCustomTiers: matched }), state);
    expect(groups[0].items[0].hint).toBe("test- · 前缀匹配");
    expect(groups[0].items[1].hint).toBe("coder · 包含匹配");
  });

  it("空段不产出：没有匹配档位时不出现空的匹配段标题", () => {
    expect(tierPickerGroups(unknownCapability, meta(), state).map((group) => group.kind)).toEqual(["builtin", "global-fallback"]);
    expect(tierPickerGroups(unknownCapability, undefined, state).map((group) => group.kind)).toEqual(["builtin", "global-fallback"]);
  });

  it("不传 settings 就不产出全局回退段：那一段的取值只能来自设置表", () => {
    expect(tierPickerGroups(unknownCapability, meta(), undefined).map((group) => group.kind)).toEqual(["builtin"]);
  });

  it("已探到不支持：摘掉匹配段，用户设定推翻不了探测事实", () => {
    const unsupported: ReasoningCapability = { ...unknownCapability, support: "unsupported" };
    const groups = tierPickerGroups(unsupported, meta({ matchedCustomTiers: matched }), state);
    expect(groups.map((group) => group.kind)).toEqual(["builtin", "global-fallback"]);
  });

  it("已探明支持：内置段来自 capability.tiers，全部可写", () => {
    const groups = tierPickerGroups(supportedCapability, meta(), state);
    const builtin = groups.find((group) => group.kind === "builtin");
    expect(builtin?.items.map((item) => item.label)).toEqual(supportedCapability.tiers.map((tier) => tier.label));
    expect(builtin?.items.every((item) => item.writable)).toBe(true);
  });

  it("未探明：内置段的可写性由 builtinTiersCompatible 三态决定，null 不算可写", () => {
    const writable = tierPickerGroups(unknownCapability, meta({ builtinTiersCompatible: true }), state);
    expect(writable.find((group) => group.kind === "builtin")?.items.every((item) => item.writable)).toBe(true);
    for (const value of [undefined, null, false] as const) {
      const groups = tierPickerGroups(unknownCapability, meta({ builtinTiersCompatible: value }), state);
      // null / undefined 是"无法确认"，标成可写就是在替探测下结论。
      expect(groups.find((group) => group.kind === "builtin")?.items.some((item) => item.writable)).toBe(false);
    }
  });

  it("档位与当前端点协议无交集时标为不可写，但不从列表里摘掉", () => {
    const groups = tierPickerGroups(unknownCapability, meta({ matchedCustomTiers: matched, supportedProtocols: ["anthropic"] }), state);
    expect(groups[0].items.map((item) => [item.id, item.writable])).toEqual([["tier-x", false], ["tier-y", true]]);
  });

  it("端点协议未探明时不断言不可写：只要档位填了参数就算可写", () => {
    const groups = tierPickerGroups(unknownCapability, meta({ matchedCustomTiers: matched, supportedProtocols: [] }), state);
    expect(groups[0].items.every((item) => item.writable)).toBe(true);
  });

  it("档位名留空时退回 id，不显示成空白", () => {
    const blank: MatchedCustomTier[] = [{ ...matched[0], label: "  " }];
    const groups = tierPickerGroups(unknownCapability, meta({ matchedCustomTiers: blank }), state);
    expect(groups[0].items[0].label).toBe("tier-x");
  });

  it("全局回退段展示旧枚举原始取值，不翻译", () => {
    for (const level of ["low", "medium", "high"] as ReasoningLevel[]) {
      const groups = tierPickerGroups(unknownCapability, meta(), { effectiveReasoningLevel: level });
      expect(groups.find((group) => group.kind === "global-fallback")?.items[0].label).toBe(level);
    }
  });
});

// —— 配置写入场景的文案。
//
// 这一组是"两处界面措辞一致"的可验证形式：文案只有这一处实现，
// `ReasoningTierPicker` 与 `ConfigPreview` 都取它的返回值。
//
// 断言的核心不是具体字句，而是**设定性取值绝不使用事实性措辞**。

describe("writeTargetSummary", () => {
  const unknownCapability: ReasoningCapability = { ...supportedCapability, support: "unknown", tiers: [], confidence: "unknown" };
  const customTier: CustomReasoningTier = { id: "tier-x", label: "超深", openaiParams: { reasoning: { effort: "xhigh" } } };
  const base = (extra: Partial<FallbackSettings> = {}): FallbackSettings =>
    ({ effectiveReasoningLevel: "medium", ...extra });

  it("已探明：场景为 builtin，档位名来自 capability.tiers", () => {
    const summary = writeTargetSummary(supportedCapability, undefined, base(), "test-coder");
    expect(summary?.scene).toBe("builtin");
    expect(summary?.tier).toBe(supportedCapability.tiers.find((tier) => tier.tier === supportedCapability.defaultTier)?.label);
    expect(summary?.custom).toBe(false);
    expect(summary?.message).toContain("已探明档位");
  });

  it("已探明且用户选了另一档：跟随选择，不停留在默认档", () => {
    const other = supportedCapability.tiers.find((tier) => tier.tier !== supportedCapability.defaultTier);
    const selection = makeSelection("test-coder", other!.tier);
    expect(writeTargetSummary(supportedCapability, undefined, base(), "test-coder", selection)?.tier).toBe(other?.label);
  });

  it("已探到不支持 / 无可用档位：不产出任何写入说明", () => {
    for (const capability of [
      { ...unknownCapability, support: "unsupported" } as ReasoningCapability,
      { ...unknownCapability, support: "supported" } as ReasoningCapability, // supported 但 tiers 为空 = empty
    ]) {
      expect(writeTargetSummary(capability, undefined, base(), "test-coder")).toBeUndefined();
    }
  });

  it("缺 settings 时不产出：全局回退档的取值只能来自设置表", () => {
    expect(writeTargetSummary(unknownCapability, undefined, undefined, "test-coder")).toBeUndefined();
  });

  it("三级兜底都映射成 matched-custom 场景：用户看到的结论一样", () => {
    const model = base({ reasoningFallbacks: [{ modelId: "test-coder", tierId: "light" }] });
    expect(writeTargetSummary(unknownCapability, undefined, model, "test-coder")).toMatchObject({
      scene: "matched-custom", tier: "轻度", custom: false,
    });
    const rule = base({
      customReasoningTiers: [customTier],
      reasoningNameRules: [{ id: "r", pattern: "test-", matchType: "prefix", tierId: "tier-x" }],
    });
    expect(writeTargetSummary(unknownCapability, undefined, rule, "test-coder")).toMatchObject({
      scene: "matched-custom", tier: "超深", custom: true,
    });
  });

  it("无任何匹配：落到 global-fallback，措辞点明可新建档位", () => {
    const summary = writeTargetSummary(unknownCapability, undefined, base(), "test-coder");
    expect(summary?.scene).toBe("global-fallback");
    expect(summary?.tier).toBe("medium");
    expect(summary?.message).toBe("配置写入：medium · 全局回退档（未探测，可新建自定义档位适配此模型）");
  });

  it("设定性场景不使用事实性措辞，已探明场景才允许说「已探明」", () => {
    const factual = ["支持", "兼容", "已确认", "已验证", "已探明"];
    for (const state of [
      base(),
      base({ reasoningFallbacks: [{ modelId: "test-coder", tierId: "light" }] }),
    ]) {
      const summary = writeTargetSummary(unknownCapability, undefined, state, "test-coder");
      for (const word of factual) expect(summary?.message).not.toContain(word);
      expect(summary?.message).toContain("未探测");
    }
    expect(writeTargetSummary(supportedCapability, undefined, base(), "test-coder")?.message).toContain("已探明");
  });

  it("每个场景都注明只影响配置写出：实时链路不发推理参数", () => {
    for (const capability of [supportedCapability, unknownCapability]) {
      const summary = writeTargetSummary(capability, undefined, base(), "test-coder");
      expect(summary?.scopeNote).toBe("仅用于写入配置文件，实时请求不发送推理参数。");
    }
  });

  it("兜底场景与 fallbackNotice 结算出同一个档位：三级降级只有一处实现", () => {
    const state = base({
      reasoningFallbacks: [{ modelId: "test-coder", tierId: "deleted" }],
      customReasoningTiers: [customTier],
      reasoningNameRules: [{ id: "r", pattern: "test-", matchType: "prefix", tierId: "tier-x" }],
    });
    expect(writeTargetSummary(unknownCapability, undefined, state, "test-coder")?.tier)
      .toBe(fallbackNotice(unknownCapability, state, "test-coder")?.tier);
  });
});
