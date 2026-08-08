import * as Switch from "@radix-ui/react-switch";
import * as Dialog from "@radix-ui/react-dialog";
import { AlertTriangle, Check, CircleX, Clipboard, Download, FileClock, Info, ListRestart, LoaderCircle, MessageCircle, RefreshCw, RotateCcw, Save, ShieldCheck, Upload, X } from "lucide-react";
import { useEffect, useState } from "react";
import type { AppSettings, ClientDescriptor, Provider, ProviderTestReport } from "../domain/types";
import { backend } from "../services/backend";
import { useAppStore } from "../state/useAppStore";

export function ClientsPage({ selected, setSelected, onPreview }: { selected: string[]; setSelected(value: string[]): void; onPreview(): void }) {
  const { clients, providers } = useAppStore();
  const current = providers.find((provider) => provider.isCurrent) ?? providers[0];
  const compatible = (client: ClientDescriptor) => current ? client.protocols.includes(current.protocol) || client.protocols.includes("custom") : false;
  return <div className="page-content"><div className="page-heading"><div><h1>客户端</h1><p>检测本机工具，并选择要应用当前服务的客户端。</p></div><button className="button primary" disabled={!current || selected.length === 0} onClick={onPreview}><Save size={17} />预览配置</button></div>
    {!current && <div className="empty-band"><Info size={20} /><span>请先添加并选择一个 Provider。</span></div>}
    <div className="client-table" role="table"><div className="table-head" role="row"><span>选择</span><span>客户端</span><span>安装状态</span><span>配置能力</span><span>兼容性</span></div>{clients.map((client) => <div className="table-row" role="row" key={client.id}><span><input aria-label={`选择 ${client.name}`} type="checkbox" checked={selected.includes(client.id)} disabled={!client.installed || !compatible(client)} onChange={(event) => setSelected(event.target.checked ? [...selected, client.id] : selected.filter((id) => id !== client.id))} /></span><span><strong>{client.name}</strong><small>{client.detectedPath ?? client.guidance}</small></span><span className={client.installed ? "positive" : "muted"}>{client.installed ? "已安装" : "未检测到"}</span><span><b className={`support ${client.support}`}>{client.support === "verified" ? "已验证" : client.support === "experimental" ? "实验性" : "手动引导"}</b></span><span>{compatible(client) ? <span className="positive"><Check size={15} />兼容</span> : <span className="muted">不兼容</span>}</span></div>)}</div>
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
    <section className="settings-section"><h2>Codex 本地兼容桥</h2><div className="setting-row"><span><ShieldCheck size={17} /><span><strong>{draft.localProxyPort ? `运行中 · 127.0.0.1:${draft.localProxyPort}` : "桌面版启动后可用"}</strong><small>仅监听本机环回地址。使用 Chat-only 服务时，请保持 Provider Deck 运行。</small></span></span></div></section>
    <section className="settings-section"><h2>剪贴板</h2><label>复制密钥后自动清除（秒）<input type="number" min={0} max={300} value={draft.clearClipboardSeconds} onChange={(e) => setDraft({ ...draft, clearClipboardSeconds: Number(e.target.value) })} /></label></section>
  </div>;
}

function SettingSwitch({ label, description, checked, onCheckedChange, danger }: { label: string; description: string; checked: boolean; onCheckedChange(value: boolean): void; danger?: boolean }) {
  return <div className="setting-row"><span>{danger && <AlertTriangle size={17} />}<span><strong>{label}</strong><small>{description}</small></span></span><Switch.Root className="switch" checked={checked} onCheckedChange={onCheckedChange}><Switch.Thumb className="switch-thumb" /></Switch.Root></div>;
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

export function ProvidersPage({ onAdd, onEdit, onApply }: { onAdd(): void; onEdit(provider: Provider): void; onApply(provider: Provider): void }) {
  const { providers, switchProvider, deleteProvider, reprobeProvider, refreshProviderModels, testProvider, operation } = useAppStore();
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
    {providers.length === 0 ? <div className="empty-state"><div className="status-icon neutral">+</div><h2>还没有服务</h2><p>添加 Base URL 和 API Key，程序会自动识别协议和模型。</p><button className="button primary" onClick={onAdd}>添加第一个服务</button></div> : <div className="provider-list">{providers.map((provider) => <article className={`provider-row ${provider.isCurrent ? "current" : ""}`} key={provider.id}><div className="provider-main"><span className={`status-dot ${provider.connectionState}`} /><div><div className="provider-title"><strong>{provider.name}</strong>{provider.isCurrent && <span className="current-label">当前</span>}<span className="protocol-badge">{provider.protocol}</span></div><code>{provider.baseUrl}</code><small>{provider.models.length} 个模型 · {provider.lastCheckedAt ? `检测于 ${new Date(provider.lastCheckedAt).toLocaleString("zh-CN")}` : "尚未检测"}</small></div></div><div className="provider-meta"><span><small>默认模型</small><strong>{provider.defaultModel ?? "未选择"}</strong></span><span><small>已应用</small><strong>{provider.appliedClients.length} 个客户端</strong></span></div><div className="row-actions"><button className="icon-button" title="重新检测" disabled={Boolean(operation)} onClick={() => reprobeProvider(provider.id)}><RefreshCw size={17} /></button><button className="button" disabled={Boolean(operation)} onClick={() => runModelRefresh(provider)}><ListRestart size={16} />获取模型</button><button className="button" disabled={Boolean(operation)} onClick={() => { setTestTarget(provider); setTestModel(provider.defaultModel ?? provider.models[0]?.id ?? ""); setReport(undefined); }}><MessageCircle size={16} />服务自测</button><button className="button" onClick={() => onEdit(provider)}>编辑</button><button className="button" onClick={() => onApply(provider)}>应用</button>{!provider.isCurrent && <button className="button" onClick={() => switchProvider(provider.id)}>设为当前</button>}<button className="icon-button danger" title="删除服务" onClick={() => { if (confirm(`删除“${provider.name}”？客户端备份不会被删除。`)) deleteProvider(provider.id); }}><span aria-hidden>×</span></button></div></article>)}</div>}
    <Dialog.Root open={Boolean(testTarget)} onOpenChange={(open) => { if (!open) { setTestTarget(undefined); setReport(undefined); } }}><Dialog.Portal><Dialog.Overlay className="dialog-overlay" /><Dialog.Content className="dialog test-dialog" aria-describedby="provider-test-description"><header className="dialog-header"><div><Dialog.Title>第三方服务自测</Dialog.Title><Dialog.Description id="provider-test-description">{testTarget?.name} · {testModel || "未选择模型"}</Dialog.Description></div><Dialog.Close className="icon-button" title="关闭"><X size={18} /></Dialog.Close></header>
      {!report ? <div className="test-intro"><div className="security-note compat-warning"><AlertTriangle size={18} /><span>将向当前第三方服务发送一条“仅回复 OK”的极短测试消息，可能产生少量费用。API Key 和完整响应不会写入报告。</span></div>{testTarget && testTarget.models.length > 0 && <label className="test-model-select">测试模型<select aria-label="测试模型" value={testModel} onChange={(event) => setTestModel(event.target.value)}>{testTarget.models.map((model) => <option value={model.id} key={model.id}>{model.displayName || model.id}</option>)}</select></label>}<p>自测依次检查模型接口连通性、身份验证和一次真实模型回复。请求不携带任何工具。</p></div> : <div className="test-report"><div className="test-summary"><strong>{report.checks.every((check) => check.status === "passed") ? "全部测试通过" : "部分测试未通过"}</strong><span>总耗时 {report.totalLatencyMs} ms</span></div>{report.checks.map((check) => <section className={`test-check ${check.status}`} key={check.id}>{check.status === "passed" ? <Check size={18} /> : <CircleX size={18} />}<div><strong>{check.label}</strong><p>{check.detail}</p>{check.latencyMs !== undefined && <small>{check.latencyMs} ms</small>}</div></section>)}{report.replyPreview && <div className="reply-preview"><small>回复摘要</small><code>{report.replyPreview}</code></div>}</div>}
      <footer className="dialog-actions"><Dialog.Close className="button">关闭</Dialog.Close><button className="button primary" disabled={Boolean(operation)} onClick={runTest}>{operation ? <LoaderCircle className="spin" size={17} /> : <MessageCircle size={17} />}{report ? "重新测试" : "开始真实对话自测"}</button></footer>
    </Dialog.Content></Dialog.Portal></Dialog.Root>
  </div>;
}
