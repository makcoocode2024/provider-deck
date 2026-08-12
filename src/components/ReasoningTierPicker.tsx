import * as Switch from "@radix-ui/react-switch";
import { AlertTriangle, Gauge, Info, LoaderCircle, RefreshCw } from "lucide-react";
import type { ReasoningBinding, ReasoningCapability, ReasoningSelection, ReasoningTier } from "../domain/types";
import {
  activeTier,
  advancedOptions,
  budgetRange,
  canDisableReasoning,
  confidenceLabel,
  constraintNotes,
  reasoningUiState,
  stateMessage,
  tierOptions,
} from "../domain/reasoning";

interface Props {
  capability?: ReasoningCapability;
  selection?: ReasoningSelection;
  onChange(next: { tier?: ReasoningTier; binding?: ReasoningBinding }): void;
  onReprobe?(): void;
  reprobing?: boolean;
}

/**
 * 推理档位选择器。
 *
 * 全部可见文字都来自后端：档位名用 `tier.label`，取值说明用 `tier.wireSummary`，
 * 结论来源用 `confidence`，约束用 `constraints.notes`。组件自己不持有任何档位词表，
 * 所以服务端新增成员时无需改动这里。
 */
export function ReasoningTierPicker({ capability, selection, onChange, onReprobe, reprobing }: Props) {
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
