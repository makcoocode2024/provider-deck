import * as Switch from "@radix-ui/react-switch";
import * as Dialog from "@radix-ui/react-dialog";
import { AlertTriangle, Check, CircleX, Clipboard, Download, FileClock, Gauge, Info, ListRestart, LoaderCircle, MessageCircle, Play, Plus, RefreshCw, RotateCcw, Save, ShieldCheck, Trash2, Upload, X } from "lucide-react";
import { useEffect, useState } from "react";
import type { AppSettings, ClientDescriptor, CustomReasoningTier, LaunchOutcome, NameMatchType, Provider, ProviderTestReport, ReasoningFallback, ReasoningLevel, ReasoningNameRule } from "../domain/types";
import { legacyReasoningLevels } from "../domain/types";
import { activeOption, builtinReasoningTiers, confidenceLabel, effectiveFallbackTier, isFallbackOrigin, originLabel, reasoningOrigin, reasoningUiState, removeFallback, resolveTierLabel, selectionFor, stateMessage, upsertFallback } from "../domain/reasoning";
import { backend } from "../services/backend";
import { useAppStore } from "../state/useAppStore";

export function ClientsPage({ selected, setSelected, onPreview }: { selected: string[]; setSelected(value: string[]): void; onPreview(): void }) {
  const { clients, providers, launchClient, operation } = useAppStore();
  const [launched, setLaunched] = useState<LaunchOutcome | undefined>();
  const [launching, setLaunching] = useState<string | undefined>();
  const current = providers.find((provider) => provider.isCurrent) ?? providers[0];
  const compatible = (client: ClientDescriptor) => current ? client.protocols.includes(current.protocol) || client.protocols.includes("custom") : false;
  // 只有客户端确认会读环境变量、且当前服务协议对得上，才把 providerId 交出去。
  // 否则纯启动——注入一个没人读的密钥只会白担一份暴露风险。
  const launch = async (client: ClientDescriptor) => {
    const injectable = client.envInjection && compatible(client) ? current?.id : undefined;
    setLaunching(client.id);
    try { setLaunched(await launchClient(client.id, injectable)); }
    catch { setLaunched(undefined); }
    finally { setLaunching(undefined); }
  };
  // 手动级别的客户端没有可写的配置文件，所以不给勾选框——勾了也只会喂给
  // 「预览配置」，让用户以为这里能写点什么。它们的入口只有「启动」。
  const configurable = (client: ClientDescriptor) => client.autoConfig;
  return <div className="page-content"><div className="page-heading"><div><h1>客户端</h1><p>检测本机工具，并选择要应用当前服务的客户端。</p></div><button className="button primary" disabled={!current || selected.length === 0} onClick={onPreview}><Save size={17} />预览配置</button></div>
    {!current && <div className="empty-band"><Info size={20} /><span>请先添加并选择一个 Provider。</span></div>}
    {launched && <div className="empty-band launch-outcome" role="status"><Info size={20} /><span><strong>{launched.clientName}</strong>：{launched.launchedPath}
      {launched.injectedVariables.length > 0 && <small>已注入环境变量：{launched.injectedVariables.join("、")}（仅变量名，值不显示也不落盘）</small>}
      {launched.warnings.map((warning) => <small key={warning}>{warning}</small>)}</span>
      <button className="button ghost" aria-label="关闭启动结果" onClick={() => setLaunched(undefined)}><X size={16} /></button></div>}
    <div className="client-table" role="table"><div className="table-head" role="row"><span>选择</span><span>客户端</span><span>安装状态</span><span>配置能力</span><span>兼容性</span><span>启动</span></div>{clients.map((client) => <div className="table-row" role="row" key={client.id}>
      <span>{configurable(client)
        ? <input aria-label={`选择 ${client.name}`} type="checkbox" checked={selected.includes(client.id)} disabled={!client.installed || !compatible(client)} onChange={(event) => setSelected(event.target.checked ? [...selected, client.id] : selected.filter((id) => id !== client.id))} />
        : <span className="muted" aria-hidden="true">—</span>}</span>
      <span><strong>{client.name}</strong><small>{client.detectedPath ?? client.guidance}</small>
        {/* 手动级别的引导文案要完整展示，不能只在没探测到路径时才露出来：
            桌面端装上了就会显示 detectedPath，而「不修改登录态」那句恰恰是装上了才需要看到。 */}
        {!configurable(client) && client.detectedPath && <small className="client-guidance">{client.guidance}</small>}
        {client.id === "codex-cli" && <small className="client-guidance">当前采用环境变量免明文配置，密钥仅在本工具拉起进程时临时注入，独立终端手动执行会提示环境变量缺失。</small>}</span>
      <span className={client.installed ? "positive" : "muted"}>{client.installed ? "已安装" : "未检测到"}</span>
      <span><b className={`support ${client.support}`}>{client.support === "verified" ? "已验证" : client.support === "experimental" ? "实验性" : "手动引导"}</b>{client.envInjection && <small>支持环境变量注入密钥</small>}</span>
      <span>{compatible(client) ? <span className="positive"><Check size={15} />兼容</span> : <span className="muted">不兼容</span>}</span>
      <span>{client.launchTarget
        ? <button className="button" disabled={Boolean(operation) || Boolean(launching) || !current} aria-label={`启动 ${client.name}`} onClick={() => launch(client)}>{launching === client.id ? <LoaderCircle className="spin" size={15} /> : <Play size={15} />}{launching === client.id ? "启动中" : "启动"}</button>
        : <span className="muted">—</span>}</span></div>)}</div>
  </div>;
}

export function BackupsPage() {
  const { backups, restore, operation } = useAppStore();
  return <div className="page-content"><div className="page-heading"><div><h1>备份与恢复</h1><p>每次写入前自动备份，可恢复到任意历史版本。</p></div></div>{backups.length === 0 ? <div className="empty-state"><FileClock size={32} /><h2>还没有备份</h2><p>首次应用配置后，备份记录会显示在这里。</p></div> : <div className="backup-list">{backups.map((backup) => <div className="backup-row" key={backup.id}><FileClock size={20} /><span><strong>{backup.clientId}</strong><small>{backup.targetPath}</small></span><time>{new Date(backup.createdAt).toLocaleString("zh-CN")}</time><button className="button" disabled={Boolean(operation)} onClick={() => restore(backup.id)}><RotateCcw size={16} />恢复</button></div>)}</div>}</div>;
}

export function SettingsPage() {
  const { settings, updateSettings } = useAppStore();
  const [draft, setDraft] = useState<AppSettings>(settings);
  const [saveState, setSaveState] = useState<"idle" | "saving" | "saved" | "failed">("idle");
  const [saveMessage, setSaveMessage] = useState("");
  useEffect(() => setDraft(settings), [settings]);
  const toggle = (key: keyof AppSettings) => (checked: boolean) => setDraft({ ...draft, [key]: checked });
  const save = async () => {
    setSaveState("saving");
    setSaveMessage("正在保存设置…");
    try {
      await updateSettings(draft);
      setSaveState("saved");
      setSaveMessage("设置已保存");
    } catch (error) {
      setSaveState("failed");
      setSaveMessage(`保存失败：${error instanceof Error ? error.message : String(error)}`);
    }
  };
  return <div className="page-content narrow"><div className="page-heading"><div><h1>设置</h1><p>控制网络、安全和配置写入策略。</p></div><button className="button primary" disabled={saveState === "saving"} onClick={save}>{saveState === "saving" ? <LoaderCircle className="spin" size={17} /> : saveState === "saved" ? <Check size={17} /> : <Save size={17} />}{saveState === "saving" ? "保存中" : "保存设置"}</button></div>
    {saveMessage && <p className={`settings-save-message ${saveState}`} role={saveState === "failed" ? "alert" : "status"}>{saveMessage}</p>}
    <section className="settings-section"><h2>配置写入</h2><SettingSwitch label="只生成配置，不自动写入" description="适合首次试用或无法确认配置格式时。" checked={draft.generateOnly} onCheckedChange={toggle("generateOnly")} /></section>
    <section className="settings-section"><h2>网络</h2><div className="form-grid"><label>请求超时（秒）<input type="number" min={3} max={120} value={draft.timeoutSeconds} onChange={(e) => setDraft({ ...draft, timeoutSeconds: Number(e.target.value) })} /></label><label>代理地址<input value={draft.proxyUrl} placeholder="留空则使用系统设置" onChange={(e) => setDraft({ ...draft, proxyUrl: e.target.value })} /></label></div><SettingSwitch label="允许自签名证书" description="默认关闭。仅可信内网服务需要启用，并会显示持续警告。" checked={draft.allowSelfSignedCertificates} onCheckedChange={toggle("allowSelfSignedCertificates")} danger /></section>
    <section className="settings-section"><h2>能力未探明时的回退档位</h2><p className="section-note">每个模型的真实推理档位由该模型自己的能力探测决定，在“编辑服务 → 确认模型”里选择。这里只管一件事：某个模型的推理能力还没探明时，配置写出该用哪个旧档位兜底。</p><label>手动回退档位<select aria-label="手动回退档位" value={draft.manualReasoningLevel} onChange={(event) => setDraft({ ...draft, manualReasoningLevel: event.target.value as ReasoningLevel })}>{legacyReasoningLevels.map((level) => <option value={level} key={level}>{level}</option>)}</select></label><div className="setting-row"><span><Gauge size={17} /><span><strong>当前回退档位：{draft.effectiveReasoningLevel}</strong><small>仅在模型推理能力未探明时使用；已探明的模型按各自能力写出。</small></span></span></div><ReasoningFallbackEditor fallbacks={draft.reasoningFallbacks} customTiers={draft.customReasoningTiers} onChange={(reasoningFallbacks) => setDraft({ ...draft, reasoningFallbacks })} /><NameRuleEditor rules={draft.reasoningNameRules} customTiers={draft.customReasoningTiers} onChange={(reasoningNameRules) => setDraft({ ...draft, reasoningNameRules })} /><CustomTierEditor tiers={draft.customReasoningTiers} rules={draft.reasoningNameRules} fallbacks={draft.reasoningFallbacks} onChange={(customReasoningTiers) => setDraft({ ...draft, customReasoningTiers })} /></section>
    <section className="settings-section"><h2>Codex 本地兼容桥</h2><div className="setting-row"><span><ShieldCheck size={17} /><span><strong>{draft.localProxyPort ? `运行中 · 127.0.0.1:${draft.localProxyPort}` : "桌面版启动后可用"}</strong><small>仅监听本机环回地址。使用 Chat-only 服务时，请保持 Provider Deck 运行。</small></span></span></div></section>
    <section className="settings-section"><h2>剪贴板</h2><label>复制密钥后自动清除（秒）<input type="number" min={0} max={300} value={draft.clearClipboardSeconds} onChange={(e) => setDraft({ ...draft, clearClipboardSeconds: Number(e.target.value) })} /></label></section>
  </div>;
}

/**
 * 逐模型兜底映射表。
 *
 * 只做"用户填什么就存什么"，不提供任何模型名建议、不做模型名匹配、不按名字推断档位——
 * 那正是第 7 节的红线。表里出现的每一个模型 ID 都必须是用户自己敲进来的。
 *
 * 添加走 `upsertFallback`（同一模型覆盖旧值），删除走 `removeFallback`：两者与后端
 * `sanitize_fallbacks` 的 last-wins 语义一致，所以界面上看到的顺序和存盘结果不会打架。
 */
function ReasoningFallbackEditor({ fallbacks, customTiers, onChange }: { fallbacks: ReasoningFallback[]; customTiers: CustomReasoningTier[]; onChange(next: ReasoningFallback[]): void }) {
  const [modelId, setModelId] = useState("");
  const [pending, setPending] = useState<string>(builtinReasoningTiers[2].id);
  const add = () => {
    if (!modelId.trim()) return;
    onChange(upsertFallback(fallbacks, { modelId, tierId: pending }));
    setModelId("");
  };
  return <div className="fallback-editor">
    <h3>逐模型兜底档位</h3>
    <p className="section-note">给某个模型单独指定兜底档位，优先于模型名规则和全局回退档。档位可以选内置的，也可以选你自建的。</p>
    <div className="fallback-add">
      <input value={modelId} placeholder="模型 ID（需与服务端返回的完全一致）" aria-label="兜底模型 ID" onChange={(event) => setModelId(event.target.value)} onKeyDown={(event) => { if (event.key === "Enter") { event.preventDefault(); add(); } }} />
      <TierSelect label="兜底档位" value={pending} customTiers={customTiers} onChange={setPending} />
      <button type="button" className="button" onClick={add} disabled={!modelId.trim()}><Plus size={16} />添加</button>
    </div>
    {fallbacks.length === 0
      ? <p className="empty-hint">尚未设置逐模型兜底，未探明的模型统一按全局回退档写出。</p>
      : <ul className="fallback-list">{fallbacks.map((item) => <li key={item.modelId}>
        <code>{item.modelId}</code>
        <TierBadge tierId={item.tierId} customTiers={customTiers} />
        <button type="button" className="button ghost" aria-label={`删除 ${item.modelId} 的兜底档位`} onClick={() => onChange(removeFallback(fallbacks, item.modelId))}><Trash2 size={15} /></button>
      </li>)}</ul>}
    <FallbackScopeNote />
  </div>;
}

/**
 * 档位下拉，内置与自定义分成两组。
 *
 * 分组是必须的：两组档位的可信程度不同——内置档位只承诺 OpenAI 系的 effort 取值，
 * 自定义档位的参数完全是用户自己写的。混在一个平铺列表里，用户分不清自己在选什么。
 */
function TierSelect({ label, value, customTiers, onChange }: { label: string; value: string; customTiers: CustomReasoningTier[]; onChange(next: string): void }) {
  const usable = customTiers.filter((tier) => tier.id.trim());
  return <select aria-label={label} value={value} onChange={(event) => onChange(event.target.value)}>
    <optgroup label="内置档位">
      {builtinReasoningTiers.map((tier) => <option value={tier.id} key={tier.id}>{tier.label}</option>)}
    </optgroup>
    {usable.length > 0 && <optgroup label="自定义档位">
      {usable.map((tier) => <option value={tier.id} key={tier.id}>{tier.label.trim() || tier.id}</option>)}
    </optgroup>}
  </select>;
}

/**
 * 档位标记。解析不到就显示原始 id 并标注「档位已删除」——不静默显示成别的档位，
 * 也不显示空白：用户需要看见这条规则当前指不到东西，才知道要去修它。
 */
function TierBadge({ tierId, customTiers }: { tierId: string; customTiers: CustomReasoningTier[] }) {
  const resolved = resolveTierLabel(tierId, customTiers);
  if (!resolved) return <span className="reasoning-badge fallback missing" title="该档位已被删除，这条规则会自动降级">{tierId || "未指定"} · 档位已删除</span>;
  return <span className="reasoning-badge fallback">{resolved.label}{resolved.custom && <em> · 自定义</em>}</span>;
}

/** 兜底作用范围的统一说明。三处兜底模块下方各挂一份，措辞必须一致。 */
function FallbackScopeNote() {
  return <p className="scope-note"><Info size={15} />仅作用于客户端配置文件写入；程序内探测、验证、本地代理功能仍以真实探明能力为准。</p>;
}

/** 三个协议参数框的元数据。顺序固定，与后端字段一一对应。 */
const protocolFields = [
  { key: "openaiParams", label: "OpenAI 协议参数", hint: '例如 {"reasoning": {"effort": "xhigh"}}' },
  { key: "anthropicParams", label: "Anthropic 协议参数", hint: '例如 {"thinking": {"type": "enabled", "budget_tokens": 8192}}' },
  { key: "geminiParams", label: "Gemini 协议参数", hint: '例如 {"generationConfig": {"thinkingConfig": {"thinkingBudget": 8192}}}' },
] as const;

type ProtocolFieldKey = (typeof protocolFields)[number]["key"];

/** 已填了参数的协议，用于列表里的标签。只看"有没有填"，不校验内容语义。 */
function supportedProtocols(tier: CustomReasoningTier): string[] {
  return protocolFields.filter((field) => tier[field.key] != null).map((field) => field.label.replace(" 协议参数", ""));
}

/**
 * 自定义档位管理。
 *
 * 存在的理由写在 types.ts 的 CustomReasoningTier 上：内置档位只能诚实表达 OpenAI 系的
 * effort，Anthropic 的 budget_tokens 和 Gemini 的 thinkingBudget 是具体数字，程序替用户
 * 编一个数字就是凭空发明取值。所以这两个协议的兜底参数只能由用户自己写。
 */
function CustomTierEditor({ tiers, rules, fallbacks, onChange }: { tiers: CustomReasoningTier[]; rules: ReasoningNameRule[]; fallbacks: ReasoningFallback[]; onChange(next: CustomReasoningTier[]): void }) {
  const [editing, setEditing] = useState<CustomReasoningTier | null>(null);
  const [removing, setRemoving] = useState<CustomReasoningTier | null>(null);
  const referenceCount = (id: string) =>
    rules.filter((rule) => rule.tierId === id).length + fallbacks.filter((item) => item.tierId === id).length;
  const save = (tier: CustomReasoningTier) => {
    const rest = tiers.filter((item) => item.id !== tier.id);
    onChange([...rest, tier]);
    setEditing(null);
  };
  return <div className="fallback-editor">
    <h3>自定义推理档位</h3>
    <p className="section-note">内置档位只能表达 OpenAI 系的 <code>reasoning.effort</code>。要为 Anthropic 或 Gemini 协议兜底，需要在这里写出该协议的原生参数——那些取值是具体数字，只有你知道自己的网关认哪个。</p>
    {tiers.length === 0
      ? <p className="empty-hint">尚未自建档位。兜底规则可以直接引用内置档位。</p>
      : <ul className="fallback-list">{tiers.map((tier) => <li key={tier.id}>
        <code>{tier.label.trim() || tier.id}</code>
        <span className="protocol-tags">{supportedProtocols(tier).map((name) => <span className="reasoning-badge fallback" key={name}>{name}</span>)}</span>
        {supportedProtocols(tier).length === 0 && <span className="reasoning-badge fallback missing">未填任何协议参数</span>}
        <button type="button" className="button ghost" aria-label={`编辑档位 ${tier.label.trim() || tier.id}`} onClick={() => setEditing(tier)}><Save size={15} /></button>
        <button type="button" className="button ghost" aria-label={`删除档位 ${tier.label.trim() || tier.id}`} onClick={() => setRemoving(tier)}><Trash2 size={15} /></button>
      </li>)}</ul>}
    <button type="button" className="button" onClick={() => setEditing({ id: `tier-${Date.now()}`, label: "", description: "", openaiParams: undefined, anthropicParams: undefined, geminiParams: undefined })}><Plus size={16} />新建档位</button>
    <FallbackScopeNote />
    {editing && <CustomTierDialog tier={editing} existing={tiers} onCancel={() => setEditing(null)} onSave={save} />}
    {removing && <DeleteTierDialog tier={removing} references={referenceCount(removing.id)} onCancel={() => setRemoving(null)} onConfirm={() => { onChange(tiers.filter((item) => item.id !== removing.id)); setRemoving(null); }} />}
  </div>;
}

/** 从模型卡片打开弹窗时带进来的规则预填。`pattern` 为空表示不预填。 */
export interface TierRuleDraft {
  pattern: string;
  matchType: NameMatchType;
}

/**
 * 新建 / 编辑档位对话框。
 *
 * 三个协议参数都是自由 JSON 文本，只校验"能不能解析"，不校验字段语义：本项目不维护
 * 各家网关的参数字典，一旦校验就等于替用户判断哪些参数合法，而那正是该由网关回答的事。
 * 参数原样存盘、原样写出，程序不改写一个字节。
 *
 * 从设置页与从模型卡片打开的是**同一个组件**：两个入口两套弹窗，校验规则和文案迟早分叉。
 * 差别只在 `prefillRule` —— 给了就多渲染一个「匹配规则」字段，`onSave` 时把规则一起交出去。
 */
export function CustomTierDialog({ tier, existing, prefillRule, onCancel, onSave }: {
  tier: CustomReasoningTier;
  existing: CustomReasoningTier[];
  /**
   * 预填的模型名匹配规则。**只是预填**：用户可以改写，也可以清空后照样保存——
   * 清空就表示"这个档位先建着，规则我自己去设置页配"，不该被强制拦住。
   */
  prefillRule?: TierRuleDraft;
  onCancel(): void;
  /** `rule` 只在渲染了规则字段且用户没清空时给出。 */
  onSave(next: CustomReasoningTier, rule?: TierRuleDraft): void;
}) {
  const [label, setLabel] = useState(tier.label);
  const [description, setDescription] = useState(tier.description ?? "");
  const [rule, setRule] = useState<TierRuleDraft | undefined>(prefillRule);
  const [text, setText] = useState<Record<ProtocolFieldKey, string>>(() => ({
    openaiParams: tier.openaiParams == null ? "" : JSON.stringify(tier.openaiParams, null, 2),
    anthropicParams: tier.anthropicParams == null ? "" : JSON.stringify(tier.anthropicParams, null, 2),
    geminiParams: tier.geminiParams == null ? "" : JSON.stringify(tier.geminiParams, null, 2),
  }));

  const parsed = protocolFields.map((field) => {
    const raw = text[field.key].trim();
    if (!raw) return { key: field.key, value: undefined, error: undefined };
    try {
      return { key: field.key, value: JSON.parse(raw) as unknown, error: undefined };
    } catch (error) {
      return { key: field.key, value: undefined, error: `JSON 无法解析：${String(error instanceof Error ? error.message : error)}` };
    }
  });
  const invalid = parsed.filter((item) => item.error);
  const filled = parsed.filter((item) => item.value !== undefined);
  const duplicateLabel = existing.some((item) => item.id !== tier.id && item.label.trim() === label.trim() && label.trim());
  const problems = [
    !label.trim() && "档位名称不能为空。",
    duplicateLabel && "已有同名档位，换一个名字以免在下拉里分不清。",
    filled.length === 0 && invalid.length === 0 && "至少填写一个协议的参数，否则这个档位引用起来只会降级。",
  ].filter((item): item is string => Boolean(item));

  const submit = () => {
    if (problems.length > 0 || invalid.length > 0) return;
    const next: CustomReasoningTier = { ...tier, label: label.trim(), description: description.trim() || null };
    for (const item of parsed) next[item.key] = item.value;
    // 规则被清成空白就当没填：空 pattern 会命中一切模型，存下去比不存危险得多。
    const trimmed = rule && rule.pattern.trim() ? { ...rule, pattern: rule.pattern.trim() } : undefined;
    onSave(next, trimmed);
  };

  return <Dialog.Root open onOpenChange={(open) => { if (!open) onCancel(); }}>
    <Dialog.Portal>
      <Dialog.Overlay className="dialog-overlay" />
      <Dialog.Content className="dialog tier-dialog" aria-label="自定义推理档位">
        <Dialog.Title>{tier.label ? "编辑档位" : "新建档位"}</Dialog.Title>
        <div className="form-grid">
          <label>档位名称<input value={label} aria-label="档位名称" placeholder="例如：超深推理" onChange={(event) => setLabel(event.target.value)} /></label>
          <label>备注<input value={description} aria-label="档位备注" placeholder="选填，仅自己看" onChange={(event) => setDescription(event.target.value)} /></label>
        </div>
        {protocolFields.map((field) => {
          const state = parsed.find((item) => item.key === field.key);
          return <label className="json-field" key={field.key}>
            {field.label}
            <textarea value={text[field.key]} aria-label={field.label} rows={4} spellCheck={false} placeholder={`留空表示该协议不使用这个档位。${field.hint}`} onChange={(event) => setText({ ...text, [field.key]: event.target.value })} />
            {state?.error && <small className="field-error">{state.error}</small>}
          </label>;
        })}
        {rule && <div className="form-grid">
          <label>模型名匹配规则<input
            value={rule.pattern}
            aria-label="模型名匹配规则"
            placeholder="留空则只建档位、不建规则"
            onChange={(event) => setRule({ ...rule, pattern: event.target.value })}
          /></label>
          <label>匹配方式<select
            aria-label="匹配方式"
            value={rule.matchType}
            onChange={(event) => setRule({ ...rule, matchType: event.target.value as NameMatchType })}
          >
            <option value="prefix">前缀匹配</option>
            <option value="contains">包含匹配</option>
          </select></label>
        </div>}
        {rule && <p className="section-note">规则预填自当前模型名，可以改写成更宽的写法覆盖同系列模型。清空后保存只建档位，不建规则。</p>}
        <p className="section-note">参数按你写的原样存盘、原样写入配置文件，程序不校验字段名也不改写取值。留空的协议在结算时跳过这一级。</p>
        {problems.map((problem) => <p className="field-error" key={problem}>{problem}</p>)}
        <div className="dialog-actions">
          <button type="button" className="button ghost" onClick={onCancel}>取消</button>
          <button type="button" className="button primary" onClick={submit} disabled={problems.length > 0 || invalid.length > 0}><Save size={16} />保存档位</button>
        </div>
      </Dialog.Content>
    </Dialog.Portal>
  </Dialog.Root>;
}

/**
 * 删除确认。必须说清后果：引用它的规则不会跟着消失，而是在结算时降级到下一级。
 * 不说明的话，用户会以为删档位等于删规则。
 */
function DeleteTierDialog({ tier, references, onCancel, onConfirm }: { tier: CustomReasoningTier; references: number; onCancel(): void; onConfirm(): void }) {
  return <Dialog.Root open onOpenChange={(open) => { if (!open) onCancel(); }}>
    <Dialog.Portal>
      <Dialog.Overlay className="dialog-overlay" />
      <Dialog.Content className="dialog tier-dialog narrow" aria-label="删除自定义档位">
        <Dialog.Title>删除档位「{tier.label.trim() || tier.id}」</Dialog.Title>
        <p className="warning-message"><AlertTriangle size={17} />关联的兜底规则将自动降级{references > 0 ? `（当前有 ${references} 条规则引用它）` : ""}。规则本身不会被删除，结算时会跳过这一级继续往下找，不会报错也不会中断配置写入。</p>
        <div className="dialog-actions">
          <button type="button" className="button ghost" onClick={onCancel}>取消</button>
          <button type="button" className="button danger" onClick={onConfirm}><Trash2 size={16} />删除档位</button>
        </div>
      </Dialog.Content>
    </Dialog.Portal>
  </Dialog.Root>;
}

/**
 * 模型名匹配规则。
 *
 * 这**不是**"根据模型名推断能力"。差别在证据来源：程序不预置任何规则，这张表初始为空，
 * 一条也不会自动生成；每一条都是用户自己写下的意图，而且只影响配置文件写出。
 * 实时请求对未探明能力照旧不发推理参数。
 *
 * 顺序即优先级，首个命中生效——所以列表按用户添加的顺序展示，不排序。
 */
function NameRuleEditor({ rules, customTiers, onChange }: { rules: ReasoningNameRule[]; customTiers: CustomReasoningTier[]; onChange(next: ReasoningNameRule[]): void }) {
  const [pattern, setPattern] = useState("");
  const [matchType, setMatchType] = useState<ReasoningNameRule["matchType"]>("prefix");
  const [tierId, setTierId] = useState<string>(builtinReasoningTiers[2].id);
  const add = () => {
    if (!pattern.trim()) return;
    onChange([...rules, { id: `rule-${Date.now()}`, pattern: pattern.trim(), matchType, tierId }]);
    setPattern("");
  };
  return <div className="fallback-editor">
    <h3>模型名匹配规则</h3>
    <p className="section-note">按模型名给未探明的模型套一个兜底档位，优先级低于逐模型兜底、高于全局回退档。匹配大小写不敏感；多条规则按下面的顺序依次尝试，第一条命中的生效。</p>
    <div className="fallback-add">
      <select aria-label="匹配方式" value={matchType} onChange={(event) => setMatchType(event.target.value as ReasoningNameRule["matchType"])}>
        <option value="prefix">前缀匹配</option>
        <option value="contains">包含匹配</option>
      </select>
      <input value={pattern} placeholder="模型名片段，例如 glm-" aria-label="匹配内容" onChange={(event) => setPattern(event.target.value)} onKeyDown={(event) => { if (event.key === "Enter") { event.preventDefault(); add(); } }} />
      <TierSelect label="规则档位" value={tierId} customTiers={customTiers} onChange={setTierId} />
      <button type="button" className="button" onClick={add} disabled={!pattern.trim()}><Plus size={16} />添加规则</button>
    </div>
    {rules.length === 0
      ? <p className="empty-hint">尚未设置任何规则。不加规则时行为与旧版本完全一致。</p>
      : <ol className="fallback-list">{rules.map((rule) => <li key={rule.id}>
        <span className="reasoning-badge fallback">{rule.matchType === "contains" ? "包含" : "前缀"}</span>
        <code>{rule.pattern}</code>
        <TierBadge tierId={rule.tierId} customTiers={customTiers} />
        <button type="button" className="button ghost" aria-label={`删除规则 ${rule.pattern}`} onClick={() => onChange(rules.filter((item) => item.id !== rule.id))}><Trash2 size={15} /></button>
      </li>)}</ol>}
    <FallbackScopeNote />
  </div>;
}

function SettingSwitch({ label, description, checked, onCheckedChange, danger }: { label: string; description: string; checked: boolean; onCheckedChange(value: boolean): void; danger?: boolean }) {
  return <div className="setting-row"><span>{danger && <AlertTriangle size={17} />}<span><strong>{label}</strong><small>{description}</small></span></span><Switch.Root className="switch" aria-label={label} checked={checked} onCheckedChange={onCheckedChange}><Switch.Thumb className="switch-thumb" /></Switch.Root></div>;
}

export function DiagnosticsPage() {
  const [diagnostics, setDiagnostics] = useState<Record<string, string>>({});
  useEffect(() => { backend.diagnostics().then(setDiagnostics).catch((error) => setDiagnostics({ error: String(error) })); }, []);
  return <div className="page-content narrow"><div className="page-heading"><div><h1>诊断信息</h1><p>以下信息不包含 API Key，可用于排查运行环境问题。</p></div><button className="button" onClick={() => navigator.clipboard.writeText(JSON.stringify(diagnostics, null, 2))}><Clipboard size={16} />复制</button></div><dl className="diagnostics">{Object.entries(diagnostics).map(([key, value]) => <div key={key}><dt>{key}</dt><dd>{value}</dd></div>)}</dl><div className="security-note"><Info size={18} /><span>分享诊断信息前仍建议人工检查路径、用户名和代理地址。</span></div></div>;
}

export function ImportExportPage() {
  const { hydrate } = useAppStore();
  const [message, setMessage] = useState("");
  const exportData = async () => { const data = await backend.exportProviders(); await navigator.clipboard.writeText(data); setMessage("非敏感配置已复制，内容不包含 API Key。"); };
  const importData = async () => { const data = window.prompt("粘贴 Provider Deck 导出的 JSON（不会读取密钥）"); if (data) { await backend.importProviders(data); await hydrate(); setMessage("配置已导入。"); } };
  return <div className="page-content narrow"><div className="page-heading"><div><h1>导入与导出</h1><p>迁移 Provider 元数据，默认且固定排除 API Key。</p></div></div><div className="action-bands"><button onClick={exportData}><Download size={22} /><span><strong>导出非敏感配置</strong><small>复制 JSON 到剪贴板，不包含任何凭据。</small></span></button><button onClick={importData}><Upload size={22} /><span><strong>导入配置</strong><small>导入后需要重新提供各服务的 API Key。</small></span></button></div>{message && <p className="success-message"><Check size={17} />{message}</p>}</div>;
}

export function AboutPage() {
  return <div className="page-content narrow"><div className="page-heading"><div><h1>关于</h1><p>Provider Deck 0.1.11</p></div></div><section className="about-block"><div className="brand-mark">PD</div><div><h2>Provider Deck</h2><p>本地优先的 AI Provider 配置与切换工具。默认无遥测，不上传 Provider、模型、URL 或密钥。</p></div></section><section className="settings-section"><h2>支持边界</h2><ul className="plain-list"><li><b>已验证：</b>Codex CLI、Claude Code、OpenCode 的公开稳定配置结构。</li><li><b>实验性：</b>Chat Completions 到 Responses 的本地兼容桥、Gemini CLI 配置生成与迁移提示。</li><li><b>手动引导：</b>VS Code、Cursor、Windsurf、Cline、Roo Code、Continue。</li><li><b>不支持：</b>修改编辑器内部数据库、长上下文撞限测试、关闭 TLS 校验。</li></ul></section></div>;
}

/**
 * 默认模型的推理现状：能力结论 / 当前选择 / 置信度。
 * 全部读后端字段，不推断——没探到就说没探到。
 */
/**
 * `settings` 是入参而不是在这里 `useAppStore()`：兜底档位的展示必须和 SettingsPage
 * 里正在编辑的那份数据同源，从组件内部另取一次容易在保存前后显示出两个不同的答案。
 */
function ReasoningSummary({ provider, settings }: { provider: Provider; settings: AppSettings }) {
  const modelId = provider.defaultModel ?? provider.models[0]?.id;
  const capability = provider.models.find((model) => model.id === modelId)?.reasoning;
  const state = reasoningUiState(capability);
  const selection = selectionFor(provider.reasoningSelections, modelId);
  const option = activeOption(capability, selection);
  const chosen = selection?.explicitBinding?.kind === "effort"
    ? `已钉死 ${selection.explicitBinding.value}`
    : option ? `${option.label}（${option.wireSummary}）` : undefined;
  // 未探明时显示的是兜底档位，不是探测结论——所以正文写兜底取值，脚注写它的来源，
  // 且这一行绝不使用 confidenceLabel：那组词描述证据强度，用在用户自填的值上是撒谎。
  const origin = reasoningOrigin(capability, settings, modelId);
  const fallbackTier = effectiveFallbackTier(capability, settings, modelId);
  if (isFallbackOrigin(origin)) {
    return <span><small>推理档位</small><strong>{stateMessage(state)}</strong><small className="fallback-note">配置写出：{fallbackTier?.label}{fallbackTier?.custom ? "（自定义）" : ""} · {originLabel(origin)}</small></span>;
  }
  return <span><small>推理档位</small><strong>{state === "supported" ? chosen ?? "未选择" : stateMessage(state)}</strong>{capability && <small>{confidenceLabel(capability.confidence)}{selection ? " · 用户已选择" : capability.defaultTier ? " · 采用默认档" : ""}</small>}</span>;
}

export function ProvidersPage({ onAdd, onEdit, onApply }: { onAdd(): void; onEdit(provider: Provider): void; onApply(provider: Provider): void }) {
  const { providers, switchProvider, deleteProvider, reprobeProvider, refreshProviderModels, testProvider, operation, settings } = useAppStore();
  const [message, setMessage] = useState("");
  const [testTarget, setTestTarget] = useState<Provider>();
  const [testModel, setTestModel] = useState("");
  const [report, setReport] = useState<ProviderTestReport>();
  const runModelRefresh = async (provider: Provider) => {
    try {
      const refreshed = await refreshProviderModels(provider.id);
      setMessage(`“${provider.name}”已更新 ${refreshed.models.length} 个模型。`);
    } catch { setMessage(""); }
  };
  const runTest = async () => {
    if (!testTarget) return;
    setReport(undefined);
    try { setReport(await testProvider(testTarget.id, testModel || undefined)); } catch { /* 全局错误提示负责展示 */ }
  };
  return <div className="page-content"><div className="page-heading"><div><h1>Provider</h1><p>管理、检测并切换本机 AI 服务。</p></div><button className="button primary" onClick={onAdd}>添加服务</button></div>
    {message && <p className="success-message" role="status"><Check size={17} />{message}</p>}
    {providers.length === 0 ? <div className="empty-state"><div className="status-icon neutral">+</div><h2>还没有服务</h2><p>添加 Base URL 和 API Key，程序会自动识别协议和模型。</p><button className="button primary" onClick={onAdd}>添加第一个服务</button></div> : <div className="provider-list">{providers.map((provider) => <article className={`provider-row ${provider.isCurrent ? "current" : ""}`} key={provider.id}><div className="provider-main"><span className={`status-dot ${provider.connectionState}`} /><div><div className="provider-title"><strong>{provider.name}</strong>{provider.isCurrent && <span className="current-label">当前</span>}<span className="protocol-badge">{provider.protocol}</span></div><code>{provider.baseUrl}</code><small>{provider.models.length} 个模型 · {provider.lastCheckedAt ? `检测于 ${new Date(provider.lastCheckedAt).toLocaleString("zh-CN")}` : "尚未检测"}</small></div></div><div className="provider-meta"><span><small>默认模型</small><strong>{provider.defaultModel ?? "未选择"}</strong></span><ReasoningSummary provider={provider} settings={settings} /><span><small>已应用</small><strong>{provider.appliedClients.length} 个客户端</strong></span></div><div className="row-actions"><button className="icon-button" title="重新检测" disabled={Boolean(operation)} onClick={() => reprobeProvider(provider.id)}><RefreshCw size={17} /></button><button className="button" disabled={Boolean(operation)} onClick={() => runModelRefresh(provider)}><ListRestart size={16} />获取模型</button><button className="button" disabled={Boolean(operation)} onClick={() => { setTestTarget(provider); setTestModel(provider.defaultModel ?? provider.models[0]?.id ?? ""); setReport(undefined); }}><MessageCircle size={16} />服务自测</button><button className="button" onClick={() => onEdit(provider)}>编辑</button><button className="button" onClick={() => onApply(provider)}>应用</button>{!provider.isCurrent && <button className="button" onClick={() => switchProvider(provider.id)}>设为当前</button>}<button className="icon-button danger" title="删除服务" onClick={() => { if (confirm(`删除“${provider.name}”？客户端备份不会被删除。`)) deleteProvider(provider.id); }}><span aria-hidden>×</span></button></div></article>)}</div>}
    <Dialog.Root open={Boolean(testTarget)} onOpenChange={(open) => { if (!open) { setTestTarget(undefined); setReport(undefined); } }}><Dialog.Portal><Dialog.Overlay className="dialog-overlay" /><Dialog.Content className="dialog test-dialog" aria-describedby="provider-test-description"><header className="dialog-header"><div><Dialog.Title>第三方服务自测</Dialog.Title><Dialog.Description id="provider-test-description">{testTarget?.name} · {testModel || "未选择模型"}</Dialog.Description></div><Dialog.Close className="icon-button" title="关闭"><X size={18} /></Dialog.Close></header>
      {!report ? <div className="test-intro"><div className="security-note compat-warning"><AlertTriangle size={18} /><span>将向当前第三方服务发送一条“仅回复 OK”的极短测试消息，可能产生少量费用。API Key 和完整响应不会写入报告。</span></div>{testTarget && testTarget.models.length > 0 && <label className="test-model-select">测试模型<select aria-label="测试模型" value={testModel} onChange={(event) => setTestModel(event.target.value)}>{testTarget.models.map((model) => <option value={model.id} key={model.id}>{model.displayName || model.id}</option>)}</select></label>}<p>自测依次检查模型接口连通性、身份验证和一次真实模型回复。请求不携带任何工具。</p></div> : <div className="test-report"><div className="test-summary"><strong>{report.checks.every((check) => check.status === "passed") ? "全部测试通过" : "部分测试未通过"}</strong><span>总耗时 {report.totalLatencyMs} ms</span></div>{report.checks.map((check) => <section className={`test-check ${check.status}`} key={check.id}>{check.status === "passed" ? <Check size={18} /> : <CircleX size={18} />}<div><strong>{check.label}</strong><p>{check.detail}</p>{check.latencyMs !== undefined && <small>{check.latencyMs} ms</small>}</div></section>)}{report.replyPreview && <div className="reply-preview"><small>回复摘要</small><code>{report.replyPreview}</code></div>}</div>}
      <footer className="dialog-actions"><Dialog.Close className="button">关闭</Dialog.Close><button className="button primary" disabled={Boolean(operation)} onClick={runTest}>{operation ? <LoaderCircle className="spin" size={17} /> : <MessageCircle size={17} />}{report ? "重新测试" : "开始真实对话自测"}</button></footer>
    </Dialog.Content></Dialog.Portal></Dialog.Root>
  </div>;
}
