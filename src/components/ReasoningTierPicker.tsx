import * as Switch from "@radix-ui/react-switch";
import { AlertTriangle, Check, Gauge, Info, LoaderCircle, Plus, RefreshCw, ShieldCheck } from "lucide-react";
import type {
  ModelReasoningMeta,
  ReasoningBinding,
  ReasoningCapability,
  ReasoningSelection,
  ReasoningTier,
  RuntimeVerification,
} from "../domain/types";
import type { TierPickerGroup, WriteTargetSummary } from "../domain/reasoning";
import {
  activeTier,
  advancedOptions,
  budgetRange,
  canDisableReasoning,
  confidenceLabel,
  constraintNotes,
  latestVerificationForTier,
  reasoningUiState,
  stateMessage,
  tierOptions,
  verifiableTier,
  verificationSummary,
  verificationTierLabel,
  verificationsFor,
} from "../domain/reasoning";

interface Props {
  capability?: ReasoningCapability;
  selection?: ReasoningSelection;
  onChange(next: { tier?: ReasoningTier; binding?: ReasoningBinding }): void;
  onReprobe?(): void;
  reprobing?: boolean;
  /**
   * 运行时验证历史，key 为 modelId。来自已保存的 `provider.reasoningVerifications`，
   * 与 `capability` **不同源**：前者是用户试过什么，后者是系统探到什么。
   */
  verifications?: Record<string, RuntimeVerification[]>;
  /** 发起一次真实验证请求。`undefined` 表示当前上下文不允许验证（例如新建流程还没有 provider）。 */
  onVerify?(tier: ReasoningTier): void;
  verifying?: boolean;
  /**
   * 配置写出会用哪个档位、这个档位从哪来。由 {@link writeTargetSummary} 算好后传入。
   *
   * 这是**用户设定或探测结论的投影**，单独一个入参、单独的样式类，绝不经过
   * `confidenceLabel`。`undefined` 表示不该显示写入说明（例如探测已排除写档位）。
   */
  writeTarget?: WriteTargetSummary;
  /**
   * 档位可选面。**只读投影**，由 `detect_model_reasoning` 返回，不含任何探测结论。
   *
   * 缺省表示还没查到额外信息，界面按只有 `capability` 时渲染——不因为缺 meta 就说"不支持"。
   */
  meta?: ModelReasoningMeta;
  /** 档位分组，由 {@link tierPickerGroups} 算好后传入。缺省则不渲染分组区。 */
  groups?: TierPickerGroup[];
  /** 正在拉取 `meta`。期间禁用档位控件，但**不清空**已有档位与选择。 */
  detecting?: boolean;
  /** 打开自定义档位编辑器。`undefined` 表示当前上下文没有编辑入口。 */
  onCreateTier?(): void;
}

/**
 * 推理档位选择器。
 *
 * 全部可见文字都来自后端：档位名用 `tier.label`，取值说明用 `tier.wireSummary`，
 * 结论来源用 `confidence`，约束用 `constraints.notes`。组件自己不持有任何档位词表，
 * 所以服务端新增成员时无需改动这里。
 */
export function ReasoningTierPicker({
  capability, selection, onChange, onReprobe, reprobing, verifications, onVerify, verifying,
  writeTarget, meta, groups, detecting, onCreateTier,
}: Props) {
  const state = reasoningUiState(capability);
  const message = stateMessage(state);

  // unknown / unavailable / empty 都是"没有可用档位"，但只有它们才配重新探测按钮。
  // unsupported 是已经探到的结论，再探一次只是浪费一次请求，所以不给按钮——
  // 两种空态必须给出不同的下一步动作，否则用户无法区分"没查到"和"确实没有"。
  if (state !== "supported") {
    return (
      <section className="reasoning-picker" aria-label="推理档位">
        <header className="reasoning-header">
          <span><Gauge size={17} /><strong>推理档位</strong></span>
          {capability && <span className="reasoning-confidence">{confidenceLabel(capability.confidence)}</span>}
        </header>
        <div className={`security-note ${state === "unsupported" ? "" : "compat-warning"}`}>
          {state === "unsupported" ? <Info size={18} /> : <AlertTriangle size={18} />}
          <span>{message}</span>
        </div>
        {/* unsupported 分支照样渲染分组区：domain 已经把 matched-custom 段摘掉了，
            剩下的内容说明"配置里会写什么"，与"这个模型支不支持推理"是两件事。 */}
        <TierGroups
          groups={groups}
          state={state}
          selection={selection}
          detecting={detecting}
          onCreateTier={onCreateTier}
        />
        <WriteTargetNote writeTarget={writeTarget} />
        {state !== "unsupported" && onReprobe && (
          <button className="button" type="button" onClick={onReprobe} disabled={reprobing}>
            {reprobing ? <LoaderCircle className="spin" size={16} /> : <RefreshCw size={16} />}重新探测
          </button>
        )}
        <ParamKindNote meta={meta} />
        <ConstraintNotes capability={capability} />
      </section>
    );
  }

  const options = tierOptions(capability);
  const current = activeTier(capability, selection);
  const budget = budgetRange(capability);
  const advanced = advancedOptions(capability);
  const toggleOnly = capability?.control.kind === "booleanToggle";
  // 能力对象自带外键，验证历史按 modelId 索引：两条数据流在这里只是并列取值，不交叉。
  const modelId = capability?.key.modelId;

  return (
    <section className="reasoning-picker" aria-label="推理档位">
      <header className="reasoning-header">
        <span><Gauge size={17} /><strong>推理档位</strong></span>
        <span className="reasoning-confidence">{confidenceLabel(capability?.confidence)}</span>
      </header>

      {toggleOnly ? (
        <BooleanToggle options={options} current={current} capability={capability} onChange={onChange} />
      ) : (
        <div className="reasoning-tier-list" role="radiogroup" aria-label="可用推理档位">
          {options.map((option) => (
            <label className="reasoning-tier-row" key={option.id}>
              <input
                type="radio"
                name="reasoning-tier"
                value={option.id}
                checked={current === option.tier}
                disabled={Boolean(detecting)}
                onChange={() => onChange({ tier: option.tier })}
              />
              <span><strong>{option.label}</strong><small>{option.wireSummary}</small></span>
            </label>
          ))}
        </div>
      )}

      {/* 已探明分支也展示匹配到的自定义档位：用户为这个模型建过档位，
          就该看见它存在，否则他会以为规则没保存成功。选中项仍由上方 radio 决定。 */}
      <TierGroups
        groups={groups}
        state={state}
        selection={selection}
        detecting={detecting}
        onCreateTier={onCreateTier}
      />
      <WriteTargetNote writeTarget={writeTarget} />

      {budget && (
        <p className="reasoning-budget-range">
          可调预算范围 {budget.min.toLocaleString()} – {budget.max.toLocaleString()} tokens
          {budget.dynamicSentinel !== undefined && <>；支持交由<b>模型自行分配</b></>}
          {!budget.offAllowed && <>；该模型不支持关闭</>}
        </p>
      )}

      {capability?.defaultReason && <small className="reasoning-default-reason">{capability.defaultReason}</small>}

      {advanced.length > 0 && (
        <details className="advanced reasoning-advanced">
          <summary>服务端声明的全部取值（{advanced.length} 个）</summary>
          <div className="reasoning-advanced-list">
            {advanced.map((value) => (
              <button
                className={`chip ${selection?.explicitBinding?.kind === "effort" && selection.explicitBinding.value === value ? "active" : ""}`}
                type="button"
                key={value}
                onClick={() => onChange({ binding: { kind: "effort", value } })}
              >{value}</button>
            ))}
          </div>
          <small>直接钉死线上取值。选择后不再随重新探测调整。</small>
        </details>
      )}

      <VerificationPanel
        capability={capability}
        selection={selection}
        verifications={verifications}
        modelId={modelId}
        onVerify={onVerify}
        verifying={verifying}
      />

      <ParamKindNote meta={meta} />
      <ConstraintNotes capability={capability} />
      {capability && capability.evidence.length > 0 && (
        <details className="advanced reasoning-evidence">
          <summary>结论依据（{capability.evidence.length} 条）</summary>
          <ul className="plain-list">
            {capability.evidence.map((item, index) => (
              <li key={`${item.source}-${index}`}>{item.detail}{item.endpoint ? ` · ${item.endpoint}` : ""}</li>
            ))}
          </ul>
        </details>
      )}
    </section>
  );
}

/**
 * 运行时验证区。与上方的能力信息刻意分成两块，各带自己的小标题：
 * 「系统探测」是 header 里的 confidence，「用户验证」是这里的三态徽章。
 * 本组件不读 `capability.confidence`，也不写任何能力字段——只投影验证历史。
 */
function VerificationPanel({ capability, selection, verifications, modelId, onVerify, verifying }: {
  capability?: ReasoningCapability;
  selection?: ReasoningSelection;
  verifications?: Record<string, RuntimeVerification[]>;
  modelId?: string;
  onVerify?(tier: ReasoningTier): void;
  verifying?: boolean;
}) {
  // 没有验证入口就整块不渲染：新建流程里后端按 provider id 查库，压根没有可验证对象。
  if (!onVerify) return null;

  // 可验证档位由 domain 判定：unsupported / unknown / 显式钉死 binding 都返回 undefined。
  const target = verifiableTier(capability, selection);
  const history = verificationsFor(verifications, modelId);
  const latest = latestVerificationForTier(verifications, modelId, target);
  const targetLabel = target ? tierOptions(capability).find((option) => option.tier === target)?.label ?? target : undefined;

  return (
    <section className="reasoning-verification" aria-label="运行时验证">
      <header className="reasoning-verification-header">
        <span><ShieldCheck size={16} /><strong>用户验证</strong></span>
        {latest ? <VerificationBadge verification={latest} capability={capability} /> : <span className="muted">尚未验证</span>}
      </header>

      {latest && <VerificationDetail verification={latest} capability={capability} />}

      <div className="reasoning-verification-actions">
        <button
          className="button"
          type="button"
          onClick={() => target && onVerify(target)}
          disabled={!target || Boolean(verifying)}
        >
          {verifying ? <LoaderCircle className="spin" size={16} /> : <Check size={16} />}
          {verifying ? "正在验证" : targetLabel ? `验证「${targetLabel}」档位` : "验证当前档位"}
        </button>
        <small>该操作会向该端点发送一次真实请求，可能产生 API 使用费用。</small>
      </div>

      {!target && (
        <small className="muted">
          已在「高级」区钉死线上取值，没有可断言的语义档位。改回档位选择后可以验证。
        </small>
      )}

      {history.length > 1 && (
        <details className="advanced reasoning-verification-history">
          <summary>验证历史（{history.length} 条）</summary>
          <ul className="plain-list">
            {history.map((record, index) => {
              const summary = verificationSummary(record, capability);
              return (
                <li key={`${record.tier}-${record.verifiedAt}-${index}`}>
                  <span className={`verification-badge ${summary.status}`}>{summary.label}</span>
                  {" · "}
                  <time dateTime={record.verifiedAt}>{new Date(record.verifiedAt).toLocaleString("zh-CN")}</time>
                  {summary.detail ? ` · ${summary.detail}` : ""}
                </li>
              );
            })}
          </ul>
          <small>验证结果只记录这一次请求的观察，不改变上方的系统探测结论。</small>
        </details>
      )}
    </section>
  );
}

/**
 * 三态徽章。文案全部来自 {@link verificationSummary}，组件不自己拼字。
 *
 * `confirmed` 的措辞是"已验证 {档位}"而不是"官方支持"或任何 confidence 词汇：
 * 它断言的只是这一次响应里出现了推理产物。
 */
function VerificationBadge({ verification, capability }: {
  verification: RuntimeVerification;
  capability?: ReasoningCapability;
}) {
  const summary = verificationSummary(verification, capability);
  return <span className={`verification-badge ${summary.status}`}>{summary.label}</span>;
}

/** 徽章下方的补充行：时间，加上 rejected 的 reason / failed 的 error。 */
function VerificationDetail({ verification, capability }: {
  verification: RuntimeVerification;
  capability?: ReasoningCapability;
}) {
  const summary = verificationSummary(verification, capability);
  return (
    <p className="reasoning-verification-detail">
      <small>
        {verificationTierLabel(verification, capability)} ·{" "}
        <time dateTime={verification.verifiedAt}>{new Date(verification.verifiedAt).toLocaleString("zh-CN")}</time>
        {summary.detail ? ` · ${summary.detail}` : ""}
      </small>
    </p>
  );
}

/** 只有开关、没有强度维度时的形态。复用既有 Switch，不另造控件。 */
function BooleanToggle({ options, current, capability, onChange }: {
  options: ReturnType<typeof tierOptions>;
  current?: ReasoningTier;
  capability?: ReasoningCapability;
  onChange: Props["onChange"];
}) {
  const on = options.find((option) => option.tier !== "off");
  const off = options.find((option) => option.tier === "off");
  const enabled = current !== undefined ? current !== "off" : true;
  const canDisable = canDisableReasoning(capability);
  const label = on?.label ?? "开启推理";
  return (
    <div className="setting-row">
      <span><span><strong>{label}</strong><small>{(enabled ? on : off)?.wireSummary ?? ""}</small></span></span>
      <Switch.Root
        className="switch"
        aria-label={label}
        checked={enabled}
        disabled={!canDisable}
        onCheckedChange={(checked) => {
          const target = checked ? on : off;
          if (target) onChange({ tier: target.tier });
        }}
      >
        <Switch.Thumb className="switch-thumb" />
      </Switch.Root>
    </div>
  );
}

/**
 * 档位分组区。三段都来自 {@link tierPickerGroups}，组件不自己筛不自己排。
 *
 * 「新建自定义档位」按钮的出现条件：未探明（`unknown` / `unavailable` / `empty`）
 * 且没有任何匹配档位。`unsupported` 一律不给入口——那是已探到的结论，
 * 建一个档位也写不出参数，给按钮等于引导用户做无效操作。
 */
function TierGroups({ groups, state, selection, detecting, onCreateTier }: {
  groups?: TierPickerGroup[];
  state: ReturnType<typeof reasoningUiState>;
  selection?: ReasoningSelection;
  detecting?: boolean;
  onCreateTier?(): void;
}) {
  if (!groups || groups.length === 0) return null;
  const matched = groups.find((group) => group.kind === "matched-custom");
  const showCreate = Boolean(onCreateTier) && state !== "unsupported" && state !== "supported" && !matched;

  // 已探明分支里，上方的 radio 列表就是内置段的可交互形态，这里再列一遍会出现
  // 两份同名档位。过滤放在组件而不是 domain：这是"谁负责渲染"的问题，不是分段规则。
  const visible = state === "supported" ? groups.filter((group) => group.kind !== "builtin") : groups;
  if (visible.length === 0) return null;

  return (
    <div className="reasoning-tier-groups">
      {showCreate && (
        <div className="reasoning-tier-create">
          <button className="button" type="button" onClick={onCreateTier} disabled={Boolean(detecting)}>
            <Plus size={16} />新建自定义档位
          </button>
          <small>为这个模型建一个档位，只影响配置文件写出。</small>
        </div>
      )}
      {visible.map((group) => (
        <section className={`reasoning-tier-group ${group.kind}`} key={group.kind} aria-label={group.label}>
          <h4>{group.label}</h4>
          <ul className="plain-list">
            {group.items.map((item) => (
              <li key={item.id} className={selection?.tier === item.id ? "active" : undefined}>
                <span><strong>{item.label}</strong>{item.hint && <small>{item.hint}</small>}</span>
                {/* 不可写如实标注，不隐藏该项：隐藏会让用户以为档位没保存成功。 */}
                {!item.writable && <em className="muted">当前端点无可写参数</em>}
              </li>
            ))}
          </ul>
        </section>
      ))}
    </div>
  );
}

/**
 * 配置写入说明。
 *
 * 措辞刻意与 confidence 徽章不同源：这里说的是"配置里会写哪一档、这一档从哪来"，
 * 而 confidence 说的是"探到的证据有多硬"。把两者写成一句话就会让用户以为
 * 自己填的档位得到了服务端确认。文案整句来自 {@link writeTargetSummary}，
 * 与 `ConfigPreview` 逐字相同——这是"两处措辞一致"唯一可验证的实现方式。
 */
export function WriteTargetNote({ writeTarget }: { writeTarget?: WriteTargetSummary }) {
  if (!writeTarget) return null;
  return (
    <p className="reasoning-fallback-note">
      <span className={`reasoning-badge ${writeTarget.scene === "builtin" ? "discovered" : "fallback"}`}>
        {writeTarget.tier}{writeTarget.custom && <em> · 自定义</em>}
      </span>
      <small>{writeTarget.message}{writeTarget.scopeNote ? ` ${writeTarget.scopeNote}` : ""}</small>
    </p>
  );
}

/**
 * 原生参数形态说明。只在探到形态时出现。
 *
 * `unknown` 不渲染任何文字：它是"探不到"，写成"不支持某种参数"就是把未探明
 * 伪装成探测结论——那正是本次要修的病。
 */
function ParamKindNote({ meta }: { meta?: ModelReasoningMeta }) {
  if (!meta || meta.nativeParamKind === "unknown") return null;
  const labels: Record<Exclude<ModelReasoningMeta["nativeParamKind"], "unknown">, string> = {
    "effort-enum": "该模型的推理参数是枚举档位，内置档位可直接使用。",
    "token-budget": "该模型的推理参数是 token 预算数值，需要自定义档位填写具体数字。",
    "boolean-toggle": "该模型的推理参数只有开关，没有强度维度。",
  };
  return <small className="reasoning-default-reason">{labels[meta.nativeParamKind]}</small>;
}

function ConstraintNotes({ capability }: { capability?: ReasoningCapability }) {
  const notes = constraintNotes(capability);
  if (notes.length === 0) return null;
  return <ul className="plain-list reasoning-constraints">{notes.map((note) => <li key={note}>{note}</li>)}</ul>;
}
