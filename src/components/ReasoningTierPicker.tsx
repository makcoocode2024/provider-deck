import * as Switch from "@radix-ui/react-switch";
import { AlertTriangle, Check, Gauge, Info, LoaderCircle, RefreshCw, ShieldCheck } from "lucide-react";
import type {
  ReasoningBinding,
  ReasoningCapability,
  ReasoningSelection,
  ReasoningTier,
  RuntimeVerification,
} from "../domain/types";
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
        {state !== "unsupported" && onReprobe && (
          <button className="button" type="button" onClick={onReprobe} disabled={reprobing}>
            {reprobing ? <LoaderCircle className="spin" size={16} /> : <RefreshCw size={16} />}重新探测
          </button>
        )}
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
                onChange={() => onChange({ tier: option.tier })}
              />
              <span><strong>{option.label}</strong><small>{option.wireSummary}</small></span>
            </label>
          ))}
        </div>
      )}

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

function ConstraintNotes({ capability }: { capability?: ReasoningCapability }) {
  const notes = constraintNotes(capability);
  if (notes.length === 0) return null;
  return <ul className="plain-list reasoning-constraints">{notes.map((note) => <li key={note}>{note}</li>)}</ul>;
}
