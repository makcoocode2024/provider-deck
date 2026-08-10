import * as Tooltip from "@radix-ui/react-tooltip";
import { ArchiveRestore, Bot, Braces, ChevronRight, CircleHelp, FileUp, Gauge, KeyRound, LayoutList, Menu, Plus, Settings, ShieldAlert, X } from "lucide-react";
import { useEffect, useState } from "react";
import type { Provider } from "./domain/types";
import { ConfigPreview } from "./components/ConfigPreview";
import { ChatHistoryDialog } from "./components/ChatHistoryDialog";
import { ProviderWizard } from "./components/ProviderWizard";
import { AboutPage, BackupsPage, ClientsPage, DiagnosticsPage, ImportExportPage, ProvidersPage, SettingsPage } from "./components/Pages";
import { useAppStore } from "./state/useAppStore";

type Page = "providers" | "clients" | "backups" | "import" | "settings" | "diagnostics" | "about";

const navGroups = [
  { label: "工作区", items: [
    { id: "providers" as Page, label: "Provider", icon: LayoutList },
    { id: "clients" as Page, label: "客户端", icon: Bot },
    { id: "backups" as Page, label: "备份与恢复", icon: ArchiveRestore },
  ] },
  { label: "工具", items: [
    { id: "import" as Page, label: "导入与导出", icon: FileUp },
    { id: "settings" as Page, label: "设置", icon: Settings },
    { id: "diagnostics" as Page, label: "诊断", icon: Gauge },
    { id: "about" as Page, label: "关于", icon: CircleHelp },
  ] },
];

export default function App() {
  const { hydrate, providers, clients, loading, operation, error, clearError, preview } = useAppStore();
  const [page, setPage] = useState<Page>("providers");
  const [wizardOpen, setWizardOpen] = useState(false);
  const [firstRun, setFirstRun] = useState(false);
  const [editing, setEditing] = useState<Provider>();
  const [selectedClients, setSelectedClients] = useState<string[]>([]);
  const [previewOpen, setPreviewOpen] = useState(false);
  const [targetProvider, setTargetProvider] = useState<Provider>();
  const [sidebarOpen, setSidebarOpen] = useState(false);
  const [chatHistoryOpen, setChatHistoryOpen] = useState(false);

  useEffect(() => { hydrate(); }, [hydrate]);
  useEffect(() => {
    if (!loading && providers.length === 0) {
      setFirstRun(true);
      setWizardOpen(true);
    }
  }, [loading, providers.length]);

  const openAdd = () => { setEditing(undefined); setFirstRun(false); setWizardOpen(true); };
  const openEdit = (provider: Provider) => { setEditing(provider); setFirstRun(false); setWizardOpen(true); };
  const beginApply = (provider: Provider) => {
    setTargetProvider(provider);
    const available = clients.filter((client) => client.installed && client.protocols.includes(provider.protocol)).map((client) => client.id);
    setSelectedClients(available);
    setPage("clients");
  };
  const showPreview = async () => {
    const provider = targetProvider ?? providers.find((item) => item.isCurrent) ?? providers[0];
    if (!provider) return;
    setTargetProvider(provider);
    await preview(provider.id, selectedClients);
    setPreviewOpen(true);
  };

  const content = () => {
    switch (page) {
      case "providers": return <ProvidersPage onAdd={openAdd} onEdit={openEdit} onApply={beginApply} />;
      case "clients": return <ClientsPage selected={selectedClients} setSelected={setSelectedClients} onPreview={showPreview} />;
      case "backups": return <BackupsPage />;
      case "import": return <ImportExportPage />;
      case "settings": return <SettingsPage />;
      case "diagnostics": return <DiagnosticsPage />;
      case "about": return <AboutPage />;
    }
  };

  return <Tooltip.Provider delayDuration={350}>
    <div className="app-shell">
      <aside className={sidebarOpen ? "sidebar open" : "sidebar"}>
        <div className="brand"><div className="brand-symbol"><Braces size={20} /></div><span><strong>Provider Deck</strong><small>本地配置中心</small></span><button className="icon-button mobile-only" title="关闭导航" onClick={() => setSidebarOpen(false)}><X size={18} /></button></div>
        <nav>{navGroups.map((group) => <div className="nav-group" key={group.label}><p>{group.label}</p>{group.items.map(({ id, label, icon: Icon }) => <button className={page === id ? "active" : ""} key={id} onClick={() => { setPage(id); setSidebarOpen(false); }}><Icon size={18} /><span>{label}</span>{page === id && <ChevronRight size={15} />}</button>)}</div>)}</nav>
        <div className="sidebar-status"><ShieldAlert size={18} /><span><strong>本地优先</strong><small>无遥测 · 凭据隔离</small></span></div>
      </aside>
      <main className="main-area">
        <header className="topbar"><button className="icon-button mobile-only" title="打开导航" onClick={() => setSidebarOpen(true)}><Menu size={19} /></button><div className="current-service"><span className={providers.some((item) => item.isCurrent) ? "status-dot connected" : "status-dot untested"} /><span><small>当前服务</small><strong>{providers.find((item) => item.isCurrent)?.name ?? "未配置"}</strong></span></div><div className="top-actions"><button className="button quiet"><KeyRound size={16} />凭据已保护</button><button className="button" onClick={() => setChatHistoryOpen(true)}><ArchiveRestore size={16} />恢复历史聊天记录</button><button className="button primary" onClick={openAdd}><Plus size={17} />添加服务</button></div></header>
        {loading ? <div className="loading-screen"><div className="brand-symbol pulse"><Braces size={22} /></div><p>正在读取本机配置状态…</p></div> : content()}
      </main>
    </div>
    {operation && <div className="operation-toast" role="status"><span className="spinner" />{operation}</div>}
    {error && <div className="error-toast" role="alert"><div><strong>操作未完成</strong><span>{error}</span></div><button className="icon-button" title="关闭" onClick={clearError}><X size={17} /></button></div>}
    <ProviderWizard open={wizardOpen} initial={editing} firstRun={firstRun} onOpenChange={setWizardOpen} onSaved={(provider) => { setTargetProvider(provider); setFirstRun(false); setWizardOpen(false); setPage("clients"); }} />
    <ConfigPreview open={previewOpen} provider={targetProvider} onOpenChange={setPreviewOpen} />
    <ChatHistoryDialog open={chatHistoryOpen} onOpenChange={setChatHistoryOpen} />
  </Tooltip.Provider>;
}
