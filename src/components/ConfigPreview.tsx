import * as Dialog from "@radix-ui/react-dialog";
import { AlertTriangle, Check, FileDiff, LoaderCircle, ShieldCheck, X } from "lucide-react";
import type { Provider } from "../domain/types";
import { useAppStore } from "../state/useAppStore";

interface Props { open: boolean; provider?: Provider; onOpenChange(open: boolean): void; }

export function ConfigPreview({ open, provider, onOpenChange }: Props) {
  const { changes, applyResults, apply, operation, settings } = useAppStore();
  const writable = changes.some((item) => item.canWrite);
  return <Dialog.Root open={open} onOpenChange={onOpenChange}><Dialog.Portal>
    <Dialog.Overlay className="dialog-overlay" />
    <Dialog.Content className="dialog preview-dialog" aria-describedby="preview-description">
      <header className="dialog-header"><div><Dialog.Title>配置变更预览</Dialog.Title><Dialog.Description id="preview-description">目标服务：{provider?.name}。写入前会重新检查文件是否被外部修改。</Dialog.Description></div><Dialog.Close className="icon-button" title="关闭"><X size={18} /></Dialog.Close></header>
      <div className="preview-list">
        {changes.map((change) => <section className="change-block" key={change.clientId}><header><div><FileDiff size={18} /><strong>{change.clientName}</strong></div><span className={`support ${change.canWrite ? "verified" : "manual"}`}>{change.canWrite ? "可自动写入" : "手动引导"}</span></header>{change.targetPath && <code className="target-path">{change.targetPath}</code>}<pre><span className="before">{change.beforePreview}</span>{"\n"}<span className="after">{change.afterPreview}</span></pre>{change.warnings.map((warning) => <p className="warning" key={warning}><AlertTriangle size={15} />{warning}</p>)}</section>)}
      </div>
      {applyResults.length > 0 && <div className="apply-results">{applyResults.map((result) => <div key={result.clientId} className={result.success ? "ok" : "warn"}>{result.success ? <Check size={17} /> : <AlertTriangle size={17} />}<span>{result.message}{result.restartRequired && "；需重启客户端"}</span></div>)}</div>}
      <div className="security-note"><ShieldCheck size={18} /><span>{settings.generateOnly ? "当前为“只生成配置”模式，不会写入文件。" : "将先创建带时间戳的备份，再执行原子写入；失败时自动回滚。"}</span></div>
      <footer className="dialog-actions split"><Dialog.Close className="button">取消</Dialog.Close><button className="button primary" disabled={!writable || Boolean(operation)} onClick={() => provider && apply(provider.id)}>{operation ? <LoaderCircle className="spin" size={17} /> : <ShieldCheck size={17} />}{settings.generateOnly ? "生成配置" : "确认备份并应用"}</button></footer>
    </Dialog.Content>
  </Dialog.Portal></Dialog.Root>;
}
