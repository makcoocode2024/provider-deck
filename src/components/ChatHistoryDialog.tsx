import * as Dialog from "@radix-ui/react-dialog";
import { AlertTriangle, ArchiveRestore, Check, Download, FileClock, LoaderCircle, RotateCcw, ShieldCheck, Upload, X } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import type { ChatBackupRecord, ChatCacheSummary, ChatRestoreMode, ChatRestoreResult } from "../domain/types";
import { backend } from "../services/backend";

type Props = { open: boolean; onOpenChange(open: boolean): void };
type Source = "file" | "cache";

type RecoveryHint = { title: string; detail: string };

function recoveryHint(message: string): RecoveryHint {
  if (/版本|版本号|不支持/.test(message)) {
    return { title: "版本不兼容", detail: "升级到最新 Provider Deck 后重试；如果暂时不能升级，请在原版本重新导出备份，不要手动编辑 JSON。" };
  }
  if (/缓存|cache|未找到|损坏|损坏/.test(message)) {
    return { title: "本地缓存不可用", detail: "先关闭正在使用 Codex 兼容桥的客户端，再重新打开 Provider Deck。仍失败时，改用“导入备份文件”，并保留错误提示与缓存路径供排查。" };
  }
  if (/解密|密钥|加密|被修改/.test(message)) {
    return { title: "备份无法解密", detail: "确认文件来自当前 Windows 用户配置，并使用 Provider Deck 导出的 .pdbchat.json 文件；跨设备或系统凭据重置后需要在原设备重新导出。" };
  }
  return { title: "文件可能已损坏", detail: "重新复制或重新导出备份，确认扩展名为 .pdbchat.json 且文件大小未被截断；不要用文本编辑器修改加密内容。" };
}

function formatSize(size: number) {
  if (size < 1024) return `${size} B`;
  if (size < 1024 * 1024) return `${(size / 1024).toFixed(1)} KB`;
  return `${(size / 1024 / 1024).toFixed(1)} MB`;
}

export function ChatHistoryDialog({ open, onOpenChange }: Props) {
  const inputRef = useRef<HTMLInputElement>(null);
  const [mode, setMode] = useState<ChatRestoreMode>("merge");
  const [source, setSource] = useState<Source>("cache");
  const [payload, setPayload] = useState("");
  const [fileName, setFileName] = useState("");
  const [summary, setSummary] = useState<ChatCacheSummary>();
  const [backups, setBackups] = useState<ChatBackupRecord[]>([]);
  const [result, setResult] = useState<ChatRestoreResult>();
  const [error, setError] = useState("");
  const [working, setWorking] = useState(false);

  const refresh = async () => {
    try {
      const [nextSummary, nextBackups] = await Promise.all([backend.chatCacheSummary(), backend.listChatBackups()]);
      setSummary(nextSummary);
      setBackups(nextBackups);
      setError("");
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    }
  };

  useEffect(() => {
    if (!open) return;
    setResult(undefined);
    setError("");
    setSource("cache");
    setPayload("");
    setFileName("");
    void refresh();
  }, [open]);

  const selectFile = async (file?: File) => {
    if (!file) return;
    setWorking(true);
    setResult(undefined);
    setError("");
    try {
      const text = await file.text();
      if (!text.trim()) throw new Error("所选备份文件为空。请重新导出完整备份。");
      setPayload(text);
      setFileName(file.name);
      setSource("file");
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    } finally {
      setWorking(false);
      if (inputRef.current) inputRef.current.value = "";
    }
  };

  const restore = async () => {
    setWorking(true);
    setResult(undefined);
    setError("");
    try {
      const next = source === "cache"
        ? await backend.restoreChatCache(mode)
        : await backend.restoreChatBackupPayload(payload, mode);
      if (next.success) {
        setResult(next);
        await refresh();
      } else {
        setError(next.message);
        setResult(next);
      }
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    } finally {
      setWorking(false);
    }
  };

  const exportBackup = async () => {
    setWorking(true);
    setError("");
    try {
      const record = await backend.exportChatBackup();
      setResult({
        success: true,
        message: `加密 JSON 备份已导出：${record.fileName}。文件位置：${record.path}`,
        importedCount: 0,
        totalCount: summary?.conversationCount ?? 0,
        currentSessionCount: summary?.currentSessionCount ?? 0,
        historicalConversationCount: summary?.historicalConversationCount ?? 0,
      });
      await refresh();
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    } finally {
      setWorking(false);
    }
  };

  const rollback = async () => {
    if (!result?.rollbackSnapshotId) return;
    setWorking(true);
    setError("");
    try {
      const next = await backend.rollbackChatRestore(result.rollbackSnapshotId);
      if (next.success) setResult(next);
      else setError(next.message);
      await refresh();
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    } finally {
      setWorking(false);
    }
  };

  const hint = error ? recoveryHint(error) : undefined;
  return <Dialog.Root open={open} onOpenChange={onOpenChange}>
    <Dialog.Portal>
      <Dialog.Overlay className="dialog-overlay" />
      <Dialog.Content className="dialog chat-dialog" aria-describedby="chat-history-description">
        <header className="dialog-header">
          <div><Dialog.Title>恢复历史聊天记录</Dialog.Title><Dialog.Description id="chat-history-description">仅恢复 Provider Deck 本地 Codex 兼容桥缓存，不会修改 Provider 配置。</Dialog.Description></div>
          <Dialog.Close className="icon-button" title="关闭"><X size={18} /></Dialog.Close>
        </header>

        <div className="chat-dialog-body">
          <div className="security-note"><ShieldCheck size={18} /><span>备份使用系统凭据加密，并以 UTF-8 JSON 容器保存。恢复前会自动创建快照，失败时可以回滚。</span></div>
          {summary && <div className="chat-summary" aria-label="聊天缓存摘要">
            <div><strong>{summary.currentSessionCount}</strong><span>当前会话</span></div>
            <div><strong>{summary.historicalConversationCount}</strong><span>历史会话</span></div>
            <div><strong>{summary.messageCount}</strong><span>消息总数</span></div>
            <span className={`chat-cache-status ${summary.cacheStatus}`}>{summary.cacheStatus === "available" ? "缓存可读" : summary.cacheStatus === "missing" ? "未找到缓存" : "缓存异常"}</span>
          </div>}

          <section className="chat-source-section">
            <h3>选择恢复来源</h3>
            <div className="chat-source-grid">
              <button className={`chat-source-card ${source === "cache" ? "selected" : ""}`} onClick={() => { setSource("cache"); setResult(undefined); }}>
                <ArchiveRestore size={20} /><span><strong>一键读取本地缓存</strong><small>读取 Provider Deck 的本地聊天缓存，适合缓存仍在但界面未加载完整的情况。</small></span>
              </button>
              <button className={`chat-source-card ${source === "file" ? "selected" : ""}`} onClick={() => { setSource("file"); setResult(undefined); inputRef.current?.click(); }}>
                <Upload size={20} /><span><strong>导入本地备份文件</strong><small>{fileName || "选择加密 .pdbchat.json 文件恢复。"}</small></span>
              </button>
            </div>
            <input ref={inputRef} className="chat-file-input" type="file" accept=".json,.pdbchat,application/json" onChange={(event) => void selectFile(event.target.files?.[0])} />
          </section>

          <section className="chat-restore-section">
            <h3>恢复策略</h3>
            <label className={`chat-mode-card ${mode === "merge" ? "selected" : ""}`}><input type="radio" name="chat-restore-mode" checked={mode === "merge"} onChange={() => setMode("merge")} /><span><strong>合并恢复（推荐）</strong><small>保留当前会话和已有历史，会话 ID 冲突时保留两份，不覆盖 Provider 配置。</small></span></label>
            <label className={`chat-mode-card replace ${mode === "replace" ? "selected" : ""}`}><input type="radio" name="chat-restore-mode" checked={mode === "replace"} onChange={() => setMode("replace")} /><span><strong>覆盖历史会话</strong><small>仅清理历史会话并导入备份；当前运行中的会话仍会保留。执行前会自动快照。</small></span></label>
          </section>

          {error && hint && <div className="chat-error-panel" role="alert"><div className="chat-error-title"><AlertTriangle size={18} /><strong>{hint.title}</strong></div><p>{error}</p><small>修复建议：{hint.detail}</small></div>}
          {result && <div className={`chat-result-panel ${result.success ? "success" : "failed"}`} role={result.success ? "status" : "alert"}><div>{result.success ? <Check size={18} /> : <AlertTriangle size={18} />}<span>{result.message}</span></div>{result.rollbackSnapshotId && <button className="button" disabled={working} onClick={() => void rollback()}><RotateCcw size={15} />一键回滚</button>}</div>}

          <section className="chat-backup-section">
            <div className="chat-section-heading"><div><h3>手动备份</h3><p>导出当前会话与历史会话的加密 JSON，建议在迁移或大版本升级前执行。</p></div><button className="button" disabled={working} onClick={() => void exportBackup()}>{working ? <LoaderCircle className="spin" size={16} /> : <Download size={16} />}导出加密 JSON</button></div>
            {backups.length > 0 && <div className="chat-backup-list">{backups.slice(0, 5).map((backup) => <div className="chat-backup-row" key={backup.id}><FileClock size={16} /><span><strong>{backup.fileName}</strong><small>{backup.conversationCount} 个会话 · {formatSize(backup.size)}</small></span><time>{new Date(backup.createdAt).toLocaleString("zh-CN")}</time></div>)}</div>}
          </section>
        </div>
        <footer className="dialog-actions chat-dialog-actions"><Dialog.Close className="button">关闭</Dialog.Close><button className={mode === "replace" ? "button danger-action" : "button primary"} disabled={working || (source === "file" && !payload)} onClick={() => void restore()}>{working ? <LoaderCircle className="spin" size={17} /> : <ArchiveRestore size={17} />}{source === "cache" ? "从本地缓存恢复" : "导入并恢复聊天记录"}</button></footer>
      </Dialog.Content>
    </Dialog.Portal>
  </Dialog.Root>;
}
