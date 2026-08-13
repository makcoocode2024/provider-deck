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
  CustomReasoningTier,
  MatchedCustomTier,
  ModelReasoningMeta,
  NameMatchType,
  ReasoningBinding,
  ReasoningCapability,
  ReasoningConfidence,
  ReasoningFallback,
  ReasoningLevel,
  ReasoningNameRule,
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

// —— 兜底档位的投影。
//
// 与上面的能力投影严格分开：能力是探测到的事实，兜底是用户在"还没探明"时给出的
// 权宜设定。两者在 UI 上必须一眼能分辨，所以这一段刻意不复用 confidenceLabels——
// 给一个用户自己填的值套上"服务端声明"之类的措辞，就是把设定伪装成事实。
//
// 这一段也不参与请求参数结算：兜底只影响配置文件写出（后端 config::codex_reasoning），
// 实时请求对未探明能力仍然省略推理参数，否则会给网关发它不认的取值。

/**
 * 内置档位的展示名，镜像后端 `ReasoningTier::label()`。
 *
 * 这里出现档位字面量是可以的：内置档位是一个封闭枚举，不随服务端声明增减。
 * 会随服务端变化的那份清单一律来自 `capability.tiers`，不在这里。
 */
const builtinTierLabels: Record<ReasoningTier, string> = {
  off: "关闭",
  light: "轻度",
  standard: "中度",
  deep: "高",
  max: "最大",
};

/** 内置档位清单，供兜底与规则的档位下拉使用（「内置档位」分组）。 */
export const builtinReasoningTiers: readonly { id: ReasoningTier; label: string }[] = (
  ["off", "light", "standard", "deep", "max"] as const
).map((id) => ({ id, label: builtinTierLabels[id] }));

/**
 * 把一个档位 id 解析成展示名，并说明它是内置的还是用户自建的。
 *
 * 与后端 `resolve_tier_config` 同样的查找顺序：先内置，再自定义。找不到返回
 * `undefined`——那不是错误，是"引用的档位已被删除"，调用方据此降级。
 */
export function resolveTierLabel(
  tierId: string | undefined,
  customTiers: CustomReasoningTier[] | undefined,
): { label: string; custom: boolean } | undefined {
  if (!tierId) return undefined;
  const builtin = builtinTierLabels[tierId as ReasoningTier];
  if (builtin) return { label: builtin, custom: false };
  const custom = (customTiers ?? []).find((tier) => tier.id === tierId);
  if (!custom) return undefined;
  return { label: custom.label.trim() || custom.id, custom: true };
}

/** 某个模型的兜底档位 id。全等匹配 modelId，与后端 `reasoning_fallback_for` 同规则。 */
export function fallbackFor(
  fallbacks: ReasoningFallback[] | undefined,
  modelId: string | undefined,
): string | undefined {
  if (!fallbacks || !modelId) return undefined;
  return fallbacks.find((item) => item.modelId === modelId)?.tierId;
}

/**
 * 首个命中的模型名规则，镜像后端 `match_name_fallback`：
 * 数组顺序即优先级，大小写不敏感，空 pattern 不参与匹配。
 *
 * 这不是"按模型名推断能力"：规则表初始为空，每一条都是用户自己写下的，
 * 而且只影响配置文件写出。
 */
export function matchNameRule(
  rules: ReasoningNameRule[] | undefined,
  modelId: string | undefined,
): ReasoningNameRule | undefined {
  if (!rules || !modelId) return undefined;
  const target = modelId.toLowerCase();
  return rules.find((rule) => {
    const pattern = rule.pattern.trim().toLowerCase();
    if (!pattern) return false;
    return rule.matchType === "contains" ? target.includes(pattern) : target.startsWith(pattern);
  });
}

/** 覆盖式写入，同一模型只留一条。空 modelId 视为无效输入，原样返回。 */
export function upsertFallback(
  fallbacks: ReasoningFallback[] | undefined,
  incoming: ReasoningFallback,
): ReasoningFallback[] {
  const modelId = incoming.modelId.trim();
  if (!modelId) return fallbacks ?? [];
  const rest = (fallbacks ?? []).filter((item) => item.modelId !== modelId);
  return [...rest, { modelId, tierId: incoming.tierId }];
}

export function removeFallback(
  fallbacks: ReasoningFallback[] | undefined,
  modelId: string,
): ReasoningFallback[] {
  return (fallbacks ?? []).filter((item) => item.modelId !== modelId);
}

/**
 * 配置写出时这个模型实际会用哪个档位，以及那个档位的来源。
 *
 * 五种来源与后端 `config::codex_reasoning` 的分支一一对应，兜底三级按优先级排列：
 * - `discovered`：能力已探明为 supported 且有可用档位，档位来自探测结果与用户选择，
 *   兜底完全不参与
 * - `model-fallback`：能力缺失或 Unknown，用户为这个模型单独设了兜底
 * - `name-rule`：能力缺失或 Unknown，没有逐模型设定，但命中了用户写的模型名规则
 * - `global-fallback`：能力缺失或 Unknown，以上都没有，走全局回退档
 * - `omitted`：探测已经给出结论，而结论排除了写档位——`unsupported`（探到不支持），
 *   或 `empty`（声明支持却没有任何可用档位）。这两种情况后端都不调用兜底闭包，
 *   用户设定推翻不了探测到的事实
 *
 * 单模型兜底压过名称规则：前者点名了这一个模型，后者是一条泛化规则，
 * 具体的意图应当胜过宽泛的意图。
 *
 * `discovered` 只回答"档位来自事实"，不回答"这个档位能否写进 Codex 的
 * model_reasoning_effort"——那要重算一遍 Adapter 的协议映射，是后端的职责，
 * 前端复算等于把同一件事实现两遍。
 */
export type ReasoningOrigin =
  | "discovered"
  | "model-fallback"
  | "name-rule"
  | "global-fallback"
  | "omitted";

/** 兜底结算需要读的三张用户配置表。故意只收这几个字段，避免调用方传整个 AppSettings。 */
export interface FallbackSettings {
  effectiveReasoningLevel: ReasoningLevel;
  reasoningFallbacks?: ReasoningFallback[];
  customReasoningTiers?: CustomReasoningTier[];
  reasoningNameRules?: ReasoningNameRule[];
}

export function reasoningOrigin(
  capability: ReasoningCapability | null | undefined,
  settings: FallbackSettings | undefined,
  modelId: string | undefined,
): ReasoningOrigin {
  const state = reasoningUiState(capability);
  if (state === "supported") return "discovered";
  if (state === "unsupported" || state === "empty") return "omitted";
  if (!settings) return "global-fallback";
  // 逐级降级：档位引用不到就当这一级不存在，继续往下找。与后端同一套规则。
  const custom = settings.customReasoningTiers;
  if (resolveTierLabel(fallbackFor(settings.reasoningFallbacks, modelId), custom)) return "model-fallback";
  const rule = matchNameRule(settings.reasoningNameRules, modelId);
  if (rule && resolveTierLabel(rule.tierId, custom)) return "name-rule";
  return "global-fallback";
}

/**
 * 来源标签。刻意与 confidenceLabels 用词不重叠：那一组描述"证据有多硬"，
 * 这一组描述"这个值是探来的还是用户填的"。
 *
 * `omitted` 的措辞不说明具体原因（不支持 / 无可用档位），因为 `stateMessage()`
 * 已经在同一处界面把原因说清了，这里再说一遍就会出现两句解释同一件事。
 */
const originLabels: Record<ReasoningOrigin, string> = {
  discovered: "已探明档位",
  "model-fallback": "兜底档位（用户为该模型设定，未探测）",
  "name-rule": "兜底档位（命中用户设定的模型名规则，未探测）",
  "global-fallback": "兜底档位（全局回退档，未探测）",
  omitted: "不写入档位（依探测结论省略）",
};

export function originLabel(origin: ReasoningOrigin): string {
  return originLabels[origin];
}

/** 该来源是否属于兜底。UI 用它决定要不要打"非探测结论"的标记。 */
export function isFallbackOrigin(origin: ReasoningOrigin): boolean {
  return origin === "model-fallback" || origin === "name-rule" || origin === "global-fallback";
}

/**
 * 配置写出时这个模型实际会用的档位，展示成一个名字。
 *
 * 只在兜底场景返回取值——已探明时档位来自 `capability.tiers` 与用户选择，
 * 由 activeOption 负责，这里不重复结算，避免两处各算一遍算出不同答案。
 *
 * 全局回退档返回旧枚举的原始取值（low/medium/high）：后端没有为那个旧枚举提供
 * 展示 label，前端就不发明一个，与设置页的下拉保持一致。
 */
export function effectiveFallbackTier(
  capability: ReasoningCapability | null | undefined,
  settings: FallbackSettings,
  modelId: string | undefined,
): { label: string; custom: boolean } | undefined {
  const origin = reasoningOrigin(capability, settings, modelId);
  const custom = settings.customReasoningTiers;
  if (origin === "model-fallback") {
    return resolveTierLabel(fallbackFor(settings.reasoningFallbacks, modelId), custom);
  }
  if (origin === "name-rule") {
    return resolveTierLabel(matchNameRule(settings.reasoningNameRules, modelId)?.tierId, custom);
  }
  if (origin === "global-fallback") return { label: settings.effectiveReasoningLevel, custom: false };
  return undefined;
}

/**
 * 兜底提示的完整内容，一次算好交给组件渲染。
 *
 * 做成一个对象而不是让组件分别调 `reasoningOrigin` + `effectiveFallbackTier`：
 * 那样两个展示位置各算一遍，将来任一处漏改就会出现"一处说兜底、一处说已探明"。
 * `undefined` 表示当前不该出现兜底提示（已探明，或探测结论已排除写档位）。
 *
 * `message` 是给用户看的一句话。措辞全部是设定性的（"兜底""仅配置生效"），
 * 不含"支持""兼容""已确认"——那些词属于探测结论。
 */
export interface ReasoningFallbackNotice {
  origin: Extract<ReasoningOrigin, "model-fallback" | "name-rule" | "global-fallback">;
  /** 档位展示名。自定义档位为用户填的名字，全局回退档为旧枚举取值。 */
  tier: string;
  /** 该档位是否是用户自建的，UI 用它决定要不要打「自定义」标记。 */
  custom: boolean;
  label: string;
  message: string;
}

export function fallbackNotice(
  capability: ReasoningCapability | null | undefined,
  settings: FallbackSettings | undefined,
  modelId: string | undefined,
): ReasoningFallbackNotice | undefined {
  if (!settings) return undefined;
  const origin = reasoningOrigin(capability, settings, modelId);
  if (origin === "discovered" || origin === "omitted") return undefined;
  const resolved = effectiveFallbackTier(capability, settings, modelId);
  if (!resolved) return undefined;
  const messages: Record<typeof origin, string> = {
    "model-fallback": `单模型兜底档位：${resolved.label}（仅配置生效）`,
    "name-rule": `名称规则兜底：${resolved.label}（仅配置生效）`,
    "global-fallback": `全局回退档：${resolved.label}（仅配置生效）`,
  };
  return {
    origin,
    tier: resolved.label,
    custom: resolved.custom,
    label: originLabel(origin),
    message: messages[origin],
  };
}

// —— 档位下拉的分组与写入场景文案。
//
// 这一段只做**展示投影**：它回答"有哪些可选项、怎么排、写入哪一档"，
// 不回答"当前生效档位是什么"——那仍然只由 activeTier / fallbackNotice 给出。
// 两处各算一遍生效档位，迟早算出不同答案。

/** 下拉里的一个可选项。`kind` 决定它属于哪一段，也决定要不要打「未探测」标记。 */
export interface TierPickerItem {
  /** 档位 id：内置档位的固定 id、自定义档位的 uuid，或全局回退档的旧枚举取值。 */
  id: string;
  label: string;
  /** 命中说明，只有 matched-custom 段有：哪条规则、什么匹配方式。 */
  hint?: string;
  /**
   * 该项在当前协议下能否写出参数。`false` 表示档位存在但这个协议没填参数。
   *
   * 不因此隐藏该项：隐藏会让用户以为档位没保存成功。如实标注让他去补参数。
   */
  writable: boolean;
}

export type TierPickerGroupKind = "matched-custom" | "builtin" | "global-fallback";

export interface TierPickerGroup {
  kind: TierPickerGroupKind;
  label: string;
  items: TierPickerItem[];
}

const groupLabels: Record<TierPickerGroupKind, string> = {
  "matched-custom": "匹配到的自定义档位",
  builtin: "内置档位",
  "global-fallback": "全局回退档",
};

/** 匹配方式的中文说明，与后端 `NameMatchType::label()` 同源同义。 */
const matchTypeLabels: Record<NameMatchType, string> = {
  prefix: "前缀匹配",
  contains: "包含匹配",
};

/**
 * 这个档位在当前端点写得出参数吗。
 *
 * 端点协议清单为空时（能力未探明）无从判断交集，退回"该档位至少填了一个协议"：
 * 未探明就断言"这个档位在此端点写不出参数"是在替探测下结论。
 *
 * 单独一个函数是因为两处要用同一条判据——下拉里的可写标注，和新建档位后
 * 该不该自动选中它。两处各写一遍迟早给出互相矛盾的答案。
 */
export function tierWritableAtEndpoint(
  meta: ModelReasoningMeta | null | undefined,
  tier: Pick<MatchedCustomTier, "supportedProtocols">,
): boolean {
  const endpointProtocols = meta?.supportedProtocols ?? [];
  return endpointProtocols.length > 0
    ? tier.supportedProtocols.some((item) => endpointProtocols.includes(item))
    : tier.supportedProtocols.length > 0;
}

/**
 * 新建档位后该不该把它设为当前模型的写入档位。
 *
 * 两个条件同时成立才算"适配这个模型"：
 * 1. 它出现在后端返回的 `matchedCustomTiers` 里 —— 也就是规则真的命中了这个模型名。
 *    命不中就自动选上，等于替用户把一条不生效的规则说成生效。
 * 2. 它在当前端点写得出参数 —— 见 {@link tierWritableAtEndpoint}。
 *
 * 返回命中的那一项而不是 boolean：调用方要用它的 `tierId` 写兜底表。
 */
export function autoSelectableTier(
  meta: ModelReasoningMeta | null | undefined,
  tierId: string,
): MatchedCustomTier | undefined {
  const hit = meta?.matchedCustomTiers.find((tier) => tier.tierId === tierId);
  if (!hit) return undefined;
  return tierWritableAtEndpoint(meta, hit) ? hit : undefined;
}

/**
 * 档位下拉的三段式分组：匹配到的自定义档位 → 内置档位 → 全局回退档。
 *
 * 三段顺序固定，且空分组不产出——出现一个空的「匹配到的自定义档位」标题，
 * 用户会以为自己的规则命中了却没显示。
 *
 * 分段依据：
 * - matched-custom 只来自后端返回的 `meta.matchedCustomTiers`，顺序原样保留（那是
 *   用户规则表的顺序，程序不替他重排）。`unsupported` 态不产出这一段——用户设定
 *   推翻不了探测到的事实。
 * - builtin 在探明支持时来自 `capability.tiers`（服务端声明的那几档），否则来自
 *   内置五档常量。后者的 `writable` 由 `meta.builtinTiersCompatible` 决定，
 *   三态里只有 `true` 才算可写：`null` 是"无法确认"，标成可写就是在替探测下结论。
 * - global-fallback 展示旧枚举原始取值（low/medium/high），与设置页下拉一致。
 *   后端没有为这个旧枚举提供展示 label，前端就不发明一个。
 *
 * 所有 label 都来自后端返回值或用户自己填的名字，本函数不自造任何档位名。
 */
export function tierPickerGroups(
  capability: ReasoningCapability | null | undefined,
  meta: ModelReasoningMeta | null | undefined,
  settings: FallbackSettings | undefined,
): TierPickerGroup[] {
  const state = reasoningUiState(capability);
  const groups: TierPickerGroup[] = [];

  if (state !== "unsupported") {
    const items: TierPickerItem[] = (meta?.matchedCustomTiers ?? []).map((tier) => ({
      id: tier.tierId,
      label: tier.label.trim() || tier.tierId,
      hint: `${tier.rulePattern} · ${matchTypeLabels[tier.ruleMatchType]}`,
      writable: tierWritableAtEndpoint(meta, tier),
    }));
    if (items.length > 0) groups.push({ kind: "matched-custom", label: groupLabels["matched-custom"], items });
  }

  const discovered = tierOptions(capability);
  const builtinItems: TierPickerItem[] = discovered.length > 0
    ? discovered.map((option) => ({ id: option.id, label: option.label, hint: option.wireSummary, writable: true }))
    : builtinReasoningTiers.map((tier) => ({
        id: tier.id,
        label: tier.label,
        writable: meta?.builtinTiersCompatible === true,
      }));
  if (builtinItems.length > 0) groups.push({ kind: "builtin", label: groupLabels.builtin, items: builtinItems });

  if (settings) {
    groups.push({
      kind: "global-fallback",
      label: groupLabels["global-fallback"],
      items: [{ id: settings.effectiveReasoningLevel, label: settings.effectiveReasoningLevel, writable: true }],
    });
  }

  return groups;
}

/** 配置写入的三种场景。由既有 {@link ReasoningOrigin} 映射而来，不新增一套判定。 */
export type WriteTargetScene = "matched-custom" | "builtin" | "global-fallback";

/**
 * 「配置里会写入哪个档位、这个档位从哪来」的唯一文案来源。
 *
 * `ReasoningTierPicker` 与 `ConfigPreview` 都调它，两处不各自拼字符串——这是
 * "两处措辞必须一致"唯一可验证的实现方式。
 *
 * `undefined` 表示不该展示任何"配置写入 X"：`omitted` 场景下探测结论已经排除写档位，
 * 此时展示一个档位名就是错的。
 *
 * 措辞规则：设定性取值（自定义档位、全局回退档）绝不出现"支持""兼容""已确认"
 * "已验证"，只有 `builtin` 场景才允许说"已探明"。
 */
export interface WriteTargetSummary {
  scene: WriteTargetScene;
  /** 档位展示名。 */
  tier: string;
  /** 该档位是否用户自建，UI 用它决定要不要打「自定义」标记。 */
  custom: boolean;
  /** 一句话结论，两处界面逐字相同。 */
  message: string;
  /** 作用范围说明。兜底场景必须写明只影响配置写出。 */
  scopeNote?: string;
}

export function writeTargetSummary(
  capability: ReasoningCapability | null | undefined,
  meta: ModelReasoningMeta | null | undefined,
  settings: FallbackSettings | undefined,
  modelId: string | undefined,
  selection?: ReasoningSelection,
): WriteTargetSummary | undefined {
  void meta;
  if (!settings) return undefined;
  const scopeNote = "仅用于写入配置文件，实时请求不发送推理参数。";

  if (reasoningOrigin(capability, settings, modelId) === "discovered") {
    const option = activeOption(capability, selection);
    // 已探明但此刻没有生效档位（用户钉死了显式 binding）时不产出场景文案：
    // 那种情况由「高级」区自己说明，这里再说一句会出现两句解释同一件事。
    if (!option) return undefined;
    return {
      scene: "builtin",
      tier: option.label,
      custom: false,
      message: `配置写入：${option.label} · 已探明档位`,
      scopeNote,
    };
  }

  // 兜底结算复用 {@link fallbackNotice}：三级降级只有一处实现，这里只换措辞。
  // `undefined` 即"探测结论已排除写档位"（unsupported / empty），不该展示任何档位名。
  const notice = fallbackNotice(capability, settings, modelId);
  if (!notice) return undefined;

  if (notice.origin === "global-fallback") {
    return {
      scene: "global-fallback",
      tier: notice.tier,
      custom: notice.custom,
      message: `配置写入：${notice.tier} · 全局回退档（未探测，可新建自定义档位适配此模型）`,
      scopeNote,
    };
  }

  // model-fallback 与 name-rule 在界面上是同一件事："命中了你自己设的档位"。
  // 两者的优先级差别只影响后端选哪一条，用户看到的结论一样。
  return {
    scene: "matched-custom",
    tier: notice.tier,
    custom: notice.custom,
    message: `配置写入：${notice.tier} · 命中你设定的档位（未探测）`,
    scopeNote,
  };
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
