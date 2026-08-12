/**
 * 推理能力的**纯展示逻辑**。
 *
 * 这里的每一个函数都只是后端 `ReasoningCapability` 的投影：档位来自 `capability.tiers`，
 * 高级取值来自 `capability.control`，文字来自后端返回的 `label` / `wireSummary`。
 * 前端不认识 low/medium/high，也不认识任何具体模型——服务端明天多声明一个 `ultra`，
 * 这里不需要改一行代码就会显示出来。
 *
 * 不 import React / invoke / store：本模块必须能在无 DOM 的单元测试里直接跑。
 */

import type {
  ReasoningBinding,
  ReasoningCapability,
  ReasoningConfidence,
  ReasoningSelection,
  ReasoningTier,
  ReasoningTierOption,
  RuntimeVerification,
  VerificationStatus,
} from "./types";

/**
 * UI 五态。把"没探到"和"探到不支持"分成两态是本模块存在的主要理由：
 * 前者应当鼓励用户重新探测，后者必须停止提供档位，两者不能混成一句"不可用"。
 */
export type ReasoningUiState =
  /** 探到支持且有可用档位 —— 显示档位选择器。 */
  | "supported"
  /** 探到明确不支持 —— 不生成任何档位。 */
  | "unsupported"
  /** 未探明 —— 不编造档位，提供重新探测。 */
  | "unknown"
  /** 声称支持但没有任何可用档位 —— 结论自相矛盾，按未探明对待但要说清。 */
  | "empty"
  /** 压根没有能力对象（模型未选、旧数据尚未探测）。 */
  | "unavailable";

export function reasoningUiState(capability?: ReasoningCapability | null): ReasoningUiState {
  if (!capability) return "unavailable";
  switch (capability.support) {
    case "unsupported": return "unsupported";
    case "supported": return capability.tiers.length > 0 ? "supported" : "empty";
    default: return "unknown";
  }
}

/** 只有 supported 才允许渲染档位选择器。 */
export function canSelectTier(capability?: ReasoningCapability | null): boolean {
  return reasoningUiState(capability) === "supported";
}

/**
 * 可选档位。**只**来自 `capability.tiers`，顺序原样保留（后端已按强弱排好）。
 * 任何情况下都不补齐、不排序、不翻译。
 */
export function tierOptions(capability?: ReasoningCapability | null): ReasoningTierOption[] {
  if (!capability || capability.support !== "supported") return [];
  return capability.tiers;
}

/**
 * 高级取值清单：effortEnum 的原始成员。用于"高级"区展示服务端到底认哪些字符串，
 * 包括没有被策展进 tiers 的成员。
 */
export function advancedOptions(capability?: ReasoningCapability | null): string[] {
  if (!capability || capability.support !== "supported") return [];
  return capability.control.kind === "effortEnum" ? capability.control.values : [];
}

/** 预算型区间，供 UI 显示"可调范围"。非预算型返回 undefined。 */
export function budgetRange(capability?: ReasoningCapability | null):
  { min: number; max: number; offAllowed: boolean; dynamicSentinel?: number } | undefined {
  if (!capability || capability.control.kind !== "tokenBudget") return undefined;
  const { min, max, offAllowed, dynamicSentinel } = capability.control;
  return { min, max, offAllowed, dynamicSentinel: dynamicSentinel ?? undefined };
}

/** 是否存在"交给模型自行分配预算"的哨兵值。 */
export function hasDynamicBudget(capability?: ReasoningCapability | null): boolean {
  return budgetRange(capability)?.dynamicSentinel !== undefined;
}

/**
 * 能否关闭推理。两个否决来源：硬约束 `constraints.cannotDisable`，
 * 以及档位里压根没有 Off 一档。任一成立就不显示关闭选项。
 */
export function canDisableReasoning(capability?: ReasoningCapability | null): boolean {
  if (!capability || capability.support !== "supported") return false;
  if (capability.constraints.cannotDisable) return false;
  return capability.tiers.some((option) => option.tier === "off");
}

/** 当前生效档位：用户选择优先，其次能力表默认档。 */
export function activeTier(
  capability?: ReasoningCapability | null,
  selection?: ReasoningSelection | null,
): ReasoningTier | undefined {
  if (selection?.explicitBinding) return undefined;
  return selection?.tier ?? capability?.defaultTier ?? undefined;
}

/** 当前生效档位对应的完整选项，找不到精确档位时不猜测。 */
export function activeOption(
  capability?: ReasoningCapability | null,
  selection?: ReasoningSelection | null,
): ReasoningTierOption | undefined {
  const tier = activeTier(capability, selection);
  if (!tier) return undefined;
  return tierOptions(capability).find((option) => option.tier === tier);
}

export function selectionFor(
  selections: ReasoningSelection[] | undefined,
  modelId: string | undefined,
): ReasoningSelection | undefined {
  if (!selections || !modelId) return undefined;
  return selections.find((item) => item.modelId === modelId);
}

/** 覆盖式写入。同一模型只保留一条选择，不累积历史。 */
export function upsertSelection(
  selections: ReasoningSelection[] | undefined,
  incoming: ReasoningSelection,
): ReasoningSelection[] {
  const rest = (selections ?? []).filter((item) => item.modelId !== incoming.modelId);
  return [...rest, incoming];
}

export function makeSelection(modelId: string, tier: ReasoningTier): ReasoningSelection {
  return { modelId, tier, source: "user", chosenAt: new Date().toISOString() };
}

export function makeBindingSelection(modelId: string, binding: ReasoningBinding): ReasoningSelection {
  return { modelId, explicitBinding: binding, source: "user", chosenAt: new Date().toISOString() };
}

/**
 * 置信度中文标签。这份映射与后端 `ReasoningConfidence::label` 同源同义，
 * 是纯粹的枚举 → 文案翻译，不含任何能力推断。
 */
const confidenceLabels: Record<ReasoningConfidence, string> = {
  unknown: "未探明",
  declared: "服务端声明",
  validated: "参数校验确认",
  // Verified confidence is reserved for future capability validation and is not
  // produced by runtime verification.
  //
  // 这一档只在后端 discovery 链路里可能出现（目前生产代码无一处写入）。用户点一次
  // 「验证」不会把 confidence 抬到这里——运行时验证的结论走 verificationSummary()，
  // 与 confidence 是两个互不相通的渠道。
  verified: "真实响应证实",
};

export function confidenceLabel(confidence?: ReasoningConfidence | null): string {
  return confidence ? confidenceLabels[confidence] : confidenceLabels.unknown;
}

/** 五态对应的空态文案。supported 无空态文案。 */
const stateMessages: Record<Exclude<ReasoningUiState, "supported">, string> = {
  unsupported: "此模型不支持推理",
  unknown: "能力未探明",
  empty: "服务端声明支持推理，但没有返回任何可用档位",
  unavailable: "尚未探测该模型的推理能力",
};

export function stateMessage(state: ReasoningUiState): string | undefined {
  return state === "supported" ? undefined : stateMessages[state];
}

/** 约束说明，直接来自后端；前端不添加解释。 */
export function constraintNotes(capability?: ReasoningCapability | null): string[] {
  return capability?.constraints.notes ?? [];
}

// —— 运行时验证的投影。
//
// 这一段只读 `Provider.reasoningVerifications`，**从不**读写 `capability.confidence`
// 或 `capability.evidence`：验证是用户行为历史，能力是系统探测事实，两者在 UI 上
// 必须能分别看到。混成一个指标就再也分不清"系统认为支持"和"我试过能用"。

/** 某个模型的验证历史。后端按时间追加，这里原样返回，不排序、不去重。 */
export function verificationsFor(
  verifications: Record<string, RuntimeVerification[]> | undefined,
  modelId: string | undefined,
): RuntimeVerification[] {
  if (!verifications || !modelId) return [];
  return verifications[modelId] ?? [];
}

/**
 * 最新一条验证记录。
 *
 * 取数组末尾而不是按 `verifiedAt` 排序：后端是追加写入，数组顺序就是权威时间顺序，
 * 不需要解析任何时间戳。RFC3339 的字典序并不等于时序——带偏移量的写法（`+08:00`）
 * 比较的是本地小时位而不是真实瞬间，一旦混入就会排错。
 */
export function latestVerification(
  verifications: Record<string, RuntimeVerification[]> | undefined,
  modelId: string | undefined,
): RuntimeVerification | undefined {
  const history = verificationsFor(verifications, modelId);
  return history.length > 0 ? history[history.length - 1] : undefined;
}

/**
 * 某个档位上最新的验证记录。
 *
 * 按档位过滤而不是只看全局最新：用户可能先验 deep 再验 light，此时"当前选中档位
 * 验过没有"才是他要的答案，而不是"最后一次验的是什么"。
 */
export function latestVerificationForTier(
  verifications: Record<string, RuntimeVerification[]> | undefined,
  modelId: string | undefined,
  tier: ReasoningTier | undefined,
): RuntimeVerification | undefined {
  if (!tier) return undefined;
  const matching = verificationsFor(verifications, modelId).filter((record) => record.tier === tier);
  return matching.length > 0 ? matching[matching.length - 1] : undefined;
}

/**
 * 可供验证的档位；`undefined` 表示此刻不该允许验证。
 *
 * 三种情况没有可验证目标：能力不支持或未探明、压根没有生效档位、以及用户在"高级"区
 * 钉死了显式 binding——后者 {@link activeTier} 按设计返回 undefined，而后端 command
 * 只接受 tier，没有可断言的档位。
 */
export function verifiableTier(
  capability?: ReasoningCapability | null,
  selection?: ReasoningSelection | null,
): ReasoningTier | undefined {
  if (reasoningUiState(capability) !== "supported") return undefined;
  return activeTier(capability, selection);
}

/**
 * 把新记录追加进验证历史，返回新对象（不改入参）。
 *
 * 追加而非覆盖，与 {@link upsertSelection} 的覆盖语义刻意相反：选择只有"当前值"，
 * 验证有"发生过什么"。Rejected / Failed 同样留痕——抹掉失败等于让用户反复点同一个
 * 按钮却看不出上次的结果。
 */
export function appendVerification(
  verifications: Record<string, RuntimeVerification[]> | undefined,
  incoming: RuntimeVerification,
): Record<string, RuntimeVerification[]> {
  const current = verifications ?? {};
  return {
    ...current,
    [incoming.modelId]: [...(current[incoming.modelId] ?? []), incoming],
  };
}

/**
 * 验证记录里的档位名。取自能力表的 `label`，能力表里没有这一档时退回后端返回的原始
 * tier 值——那也是后端数据，不是前端自造的词。
 */
export function verificationTierLabel(
  verification: RuntimeVerification,
  capability?: ReasoningCapability | null,
): string {
  return tierOptions(capability).find((option) => option.tier === verification.tier)?.label
    ?? verification.tier;
}

export interface VerificationSummary {
  status: VerificationStatus;
  /** 主文案，可直接展示。 */
  label: string;
  /** 补充说明：rejected 的 reason、failed 的 error。confirmed 没有。 */
  detail?: string;
}

/**
 * 三态文案。
 *
 * 三条文案都刻意避开"不支持"二字：`rejected` 说的是"这一次响应里没看到推理产物"，
 * `failed` 说的是"这一次请求没走通"，两者都不构成能力结论。能力是否支持只由
 * {@link reasoningUiState} 回答，它读的是 `capability.support`。
 */
export function verificationSummary(
  verification: RuntimeVerification,
  capability?: ReasoningCapability | null,
): VerificationSummary {
  const tierName = verificationTierLabel(verification, capability);
  switch (verification.result.status) {
    case "confirmed":
      return { status: "confirmed", label: `已验证 ${tierName}` };
    case "rejected":
      return {
        status: "rejected",
        label: `此 endpoint 下「${tierName}」未检测到推理产物`,
        detail: verification.result.reason,
      };
    case "failed":
      return { status: "failed", label: "验证失败", detail: verification.result.error };
  }
}
