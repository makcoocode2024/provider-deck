import * as Dialog from "@radix-ui/react-dialog";
import { Check, ChevronLeft, Eye, EyeOff, LoaderCircle, Search, ShieldCheck, X } from "lucide-react";
import { useEffect, useState } from "react";
import { useForm } from "react-hook-form";
import { z } from "zod";
import type { ClaudeModelMappings, ClaudeModelProfile, CodexCompatibility, ModelInfo, Provider, ProviderDraft, ReasoningBinding, ReasoningTier } from "../domain/types";
import { makeBindingSelection, makeSelection, selectionFor, upsertSelection } from "../domain/reasoning";
import { normalizeBaseUrl } from "../domain/url";
import { backend } from "../services/backend";
import { useAppStore } from "../state/useAppStore";
import { ReasoningTierPicker } from "./ReasoningTierPicker";

const emptyToUndefined = (value: unknown) => value === null || value === undefined || value === "" ? undefined : value;
const optionalString = z.preprocess(emptyToUndefined, z.string().trim().optional());

type ReasoningSelectionShape = NonNullable<ProviderDraft["reasoningSelections"]>[number];

const schema = z.object({
  id: z.string().optional(),
  name: z.string().trim().min(1, "请输入服务名称").max(60),
  baseUrl: z.string().trim().min(1, "请输入 Base URL").refine((value) => {
    try { normalizeBaseUrl(value); return true; } catch { return false; }
  }, "Base URL 格式无效"),
  apiKey: z.string(),
  protocolHint: z.preprocess(
    emptyToUndefined,
    z.enum(["openai", "anthropic", "gemini", "azure-openai", "custom"]).optional(),
  ),
  timeoutSeconds: z.number().min(3).max(120),
  azureApiVersion: optionalString,
  defaultModel: optionalString,
  claudeModelProfile: z.preprocess(emptyToUndefined, z.enum(["sonnet", "opus", "haiku"]).optional()),
  claudeExtendedContext: z.preprocess((value) => value === null ? undefined : value, z.boolean().optional()),
  claudeModelMappings: z.preprocess(
    (value) => value === null ? undefined : value,
    z.object({ sonnet: optionalString, opus: optionalString, haiku: optionalString }).optional(),
  ),
  // 选择是后端结构的原样回传，前端不校验其内容，只保证它不被 zod 剥掉。
  reasoningSelections: z.array(z.custom<ReasoningSelectionShape>()).optional(),
}).superRefine((draft, context) => {
  if (!draft.id && !draft.apiKey.trim()) {
    context.addIssue({ code: "custom", path: ["apiKey"], message: "请输入 API Key" });
  }
});

const inferClaudeProfile = (model?: string): ClaudeModelProfile => {
  const normalized = model?.toLowerCase() ?? "";
  if (normalized.includes("opus")) return "opus";
  if (normalized.includes("haiku")) return "haiku";
  return "sonnet";
};

const claudeProfiles: Array<{ id: ClaudeModelProfile; label: string }> = [
  { id: "sonnet", label: "Sonnet" },
  { id: "opus", label: "Opus" },
  { id: "haiku", label: "Haiku" },
];

const fieldLabels: Record<string, string> = {
  name: "服务名称",
  baseUrl: "Base URL",
  apiKey: "API Key",
  protocolHint: "协议提示",
  timeoutSeconds: "超时",
  azureApiVersion: "Azure API 版本",
  defaultModel: "默认模型",
  claudeModelProfile: "Claude Code 模型档位",
  claudeExtendedContext: "上下文窗口",
  claudeModelMappings: "Claude Code 模型映射",
};

const codexCompatibilityCopy: Record<CodexCompatibility, { title: string; detail: string; tone: "success" | "warning" }> = {
  full: { title: "Codex 完整兼容", detail: "Responses API 和 custom 工具均通过探测，将保留完整补丁能力。", tone: "success" },
  "function-tools-only": { title: "Codex 已自动兼容降级", detail: "网关不接受 custom 工具，配置将关闭自由格式补丁声明并使用 Codex 默认兼容工具。", tone: "warning" },
  "chat-proxy": { title: "Codex 本地兼容桥已启用", detail: "此服务只支持 Chat Completions。Provider Deck 会在本机转换 Codex 请求；使用 Codex 时需保持本程序运行，内置搜索、文件输入和 namespace 等高级能力可能不可用。", tone: "warning" },
  "responses-unsupported": { title: "暂不能用于当前 Codex", detail: "网关缺少 Codex 必需的 Responses 工具协议，程序不会写入无效配置。", tone: "warning" },
  unknown: { title: "Codex 采用保守模式", detail: "探测因网络、限流或服务异常未完成，配置将关闭自由格式补丁声明。", tone: "warning" },
  "not-applicable": { title: "无需 Codex 探测", detail: "当前协议不适用于 Codex CLI。", tone: "success" },
};

const assignClaudeMapping = (mappings: ClaudeModelMappings, profile: ClaudeModelProfile, modelId?: string): ClaudeModelMappings => {
  const next = { ...mappings };
  if (modelId) {
    const previous = next[profile];
    const occupied = claudeProfiles.find((item) => item.id !== profile && next[item.id] === modelId);
    if (occupied) next[occupied.id] = previous && previous !== modelId ? previous : undefined;
  }
  next[profile] = modelId || undefined;
  return next;
};

const normalizeClaudeMappings = (models: ModelInfo[], defaultModel?: string, profile?: ClaudeModelProfile, existing?: ClaudeModelMappings): ClaudeModelMappings => {
  const available = new Set(models.map((model) => model.id));
  const mappings: ClaudeModelMappings = {
    sonnet: existing?.sonnet && available.has(existing.sonnet) ? existing.sonnet : undefined,
    opus: existing?.opus && available.has(existing.opus) ? existing.opus : undefined,
    haiku: existing?.haiku && available.has(existing.haiku) ? existing.haiku : undefined,
  };
  const selectedProfile = profile ?? inferClaudeProfile(defaultModel);
  const selectedMappings = defaultModel && available.has(defaultModel) ? assignClaudeMapping(mappings, selectedProfile, defaultModel) : mappings;
  const used = new Set(Object.values(selectedMappings).filter((model): model is string => Boolean(model)));
  for (const model of models) {
    if (used.has(model.id)) continue;
    const preferred = inferClaudeProfile(model.id);
    if (!selectedMappings[preferred]) {
      selectedMappings[preferred] = model.id;
      used.add(model.id);
      continue;
    }
    const empty = claudeProfiles.find((item) => !selectedMappings[item.id]);
    if (!empty) break;
    selectedMappings[empty.id] = model.id;
    used.add(model.id);
  }
  const fallback = defaultModel && available.has(defaultModel) ? defaultModel : models[0]?.id;
  if (fallback) {
    for (const profile of claudeProfiles) {
      if (!selectedMappings[profile.id]) selectedMappings[profile.id] = fallback;
    }
  }
  return selectedMappings;
};

interface Props {
  open: boolean;
  initial?: Provider;
  firstRun?: boolean;
  onOpenChange(open: boolean): void;
  onSaved(provider: Provider): void;
}

export function ProviderWizard({ open, initial, firstRun, onOpenChange, onSaved }: Props) {
  const [step, setStep] = useState<"form" | "detect" | "models">("form");
  const [showKey, setShowKey] = useState(false);
  const [keyState, setKeyState] = useState<"idle" | "loading" | "loaded" | "failed">("idle");
  const [keyError, setKeyError] = useState("");
  const [formError, setFormError] = useState("");
  const [startingProbe, setStartingProbe] = useState(false);
  const [reprobingReasoning, setReprobingReasoning] = useState(false);
  const [verifyingReasoning, setVerifyingReasoning] = useState(false);
  const { probe, saveProvider, reprobeModelReasoning, verifyModelReasoning, probeResult, operation, error, clearError } = useAppStore();
  // 验证历史取自 store 里已保存的 provider，而不是 initial：验证会往 store 追加记录，
  // 读 initial 就看不到刚刚那一条。capability 仍走 probeResult，两条数据流各自的源不变。
  const savedProvider = useAppStore((state) => state.providers.find((item) => item.id === initial?.id));
  const form = useForm<ProviderDraft>({
    defaultValues: {
      id: initial?.id,
      name: initial?.name ?? "",
      baseUrl: initial?.baseUrl ?? "",
      apiKey: "",
      protocolHint: initial?.protocol,
      timeoutSeconds: 10,
      defaultModel: initial?.defaultModel,
      claudeModelProfile: initial?.claudeModelProfile ?? inferClaudeProfile(initial?.defaultModel),
      claudeExtendedContext: initial?.claudeExtendedContext ?? false,
      claudeModelMappings: initial?.claudeModelMappings,
      reasoningSelections: initial?.reasoningSelections ?? [],
    },
  });

  useEffect(() => {
    let cancelled = false;
    if (open) {
      setStep("form");
      setShowKey(false);
      setKeyError("");
      setFormError("");
      setStartingProbe(false);
      clearError();
      form.reset({
        id: initial?.id,
        name: initial?.name ?? "",
        baseUrl: initial?.baseUrl ?? "",
        apiKey: "",
        protocolHint: initial?.protocol,
        timeoutSeconds: 10,
        defaultModel: initial?.defaultModel,
        claudeModelProfile: initial?.claudeModelProfile ?? inferClaudeProfile(initial?.defaultModel),
        claudeExtendedContext: initial?.claudeExtendedContext ?? false,
        claudeModelMappings: initial?.claudeModelMappings,
        reasoningSelections: initial?.reasoningSelections ?? [],
      });
      if (initial?.id) {
        setKeyState("loading");
        backend.getProviderApiKey(initial.id).then((apiKey) => {
          if (cancelled) return;
          form.setValue("apiKey", apiKey, { shouldValidate: true });
          form.clearErrors("apiKey");
          setKeyState("loaded");
        }).catch((loadError) => {
          if (cancelled) return;
          setKeyState("failed");
          setKeyError(loadError instanceof Error ? loadError.message : String(loadError));
        });
      } else {
        setKeyState("idle");
      }
    }
    return () => { cancelled = true; };
  }, [open, initial, form, clearError]);

  const runProbe = async () => {
    if (startingProbe || operation) return;
    setFormError("");
    form.clearErrors();
    const draft = form.getValues();
    const parsed = schema.safeParse(draft);
    if (!parsed.success) {
      const messages: string[] = [];
      for (const issue of parsed.error.issues) {
        const field = issue.path[0];
        if (typeof field === "string") {
          form.setError(field as keyof ProviderDraft, { message: issue.message });
          messages.push(`${fieldLabels[field] ?? field}：${issue.message}`);
        } else {
          messages.push(issue.message);
        }
      }
      setFormError(messages.length > 0
        ? `请检查以下信息：${[...new Set(messages)].join("；")}`
        : "服务信息校验失败，请检查后重试。");
      return;
    }
    setStartingProbe(true);
    setStep("detect");
    try {
      const result = await probe(parsed.data);
      const defaultModel = parsed.data.defaultModel && result.models.some((model) => model.id === parsed.data.defaultModel) ? parsed.data.defaultModel : result.models[0]?.id;
      if (defaultModel) form.setValue("defaultModel", defaultModel);
      if (result.protocol === "anthropic") {
        form.setValue("claudeModelMappings", normalizeClaudeMappings(result.models, defaultModel, form.getValues("claudeModelProfile"), form.getValues("claudeModelMappings")));
      }
      setStep("models");
    } catch {
      // The store renders the backend error in the detection step.
    } finally {
      setStartingProbe(false);
    }
  };

  const save = async () => {
    try {
      const provider = await saveProvider(form.getValues());
      onSaved(provider);
      onOpenChange(false);
    } catch {
      // The store exposes the backend error toast; keep the form available for retry or close.
    }
  };

  const detectedProtocol = probeResult?.protocol ?? form.watch("protocolHint");
  const claudeProfile = form.watch("claudeModelProfile") ?? "sonnet";
  const claudeMappings = form.watch("claudeModelMappings") ?? {};
  const detectedModels = probeResult?.models ?? [];
  const selectModel = (modelId: string) => {
    form.setValue("defaultModel", modelId);
    if (detectedProtocol === "anthropic") {
      const profile = form.getValues("claudeModelProfile") ?? inferClaudeProfile(modelId);
      form.setValue("claudeModelMappings", assignClaudeMapping(form.getValues("claudeModelMappings") ?? {}, profile, modelId));
    }
  };
  const selectClaudeMapping = (profile: ClaudeModelProfile, modelId: string) => {
    form.setValue("claudeModelMappings", assignClaudeMapping(form.getValues("claudeModelMappings") ?? {}, profile, modelId));
  };

  // 推理能力挂在探测回来的 ModelInfo 上，选择挂在草稿上：切换默认模型时能力随之改变，
  // 但其他模型已经做过的选择必须留着——用户在两个模型间来回比较不应该丢掉任何一次选择。
  const activeModelId = form.watch("defaultModel") ?? detectedModels[0]?.id;
  const activeModel = detectedModels.find((model) => model.id === activeModelId);
  const draftSelections = form.watch("reasoningSelections") ?? [];
  const activeSelection = selectionFor(draftSelections, activeModelId);
  const changeReasoning = (next: { tier?: ReasoningTier; binding?: ReasoningBinding }) => {
    if (!activeModelId) return;
    const selection = next.binding
      ? makeBindingSelection(activeModelId, next.binding)
      : next.tier ? makeSelection(activeModelId, next.tier) : undefined;
    if (!selection) return;
    form.setValue("reasoningSelections", upsertSelection(form.getValues("reasoningSelections"), selection));
  };
  const reprobeReasoning = async () => {
    if (!initial?.id || !activeModelId) return;
    setReprobingReasoning(true);
    try { await reprobeModelReasoning(initial.id, activeModelId); }
    finally { setReprobingReasoning(false); }
  };
  // 验证需要一个已入库的 provider：后端 command 按 id 查 StateStore 并读取凭据，
  // 新建流程还没有 id，所以那时不传 onVerify，整个验证区不渲染。
  const verifyReasoning = async (tier: ReasoningTier) => {
    if (!initial?.id || !activeModelId) return;
    setVerifyingReasoning(true);
    try { await verifyModelReasoning(initial.id, activeModelId, tier); }
    catch { /* store 已经把错误放进全局 toast，这里只负责收尾 loading。 */ }
    finally { setVerifyingReasoning(false); }
  };

  return (
    <Dialog.Root open={open} onOpenChange={onOpenChange}>
      <Dialog.Portal>
        <Dialog.Overlay className="dialog-overlay" />
        <Dialog.Content className="dialog wizard" aria-describedby="provider-description">
          <header className="dialog-header">
            <div>
              <Dialog.Title>{firstRun ? "添加第一个 AI 服务" : initial ? "编辑服务" : "添加服务"}</Dialog.Title>
              <Dialog.Description id="provider-description">
                {step === "form" && "填写两个关键信息，剩余部分由程序自动检测。"}
                {step === "detect" && "读取模型元数据；OpenAI-compatible 服务会额外发送最多两次 1-token 工具兼容性请求。"}
                {step === "models" && "确认协议与模型后保存，密钥将进入系统凭据库。"}
              </Dialog.Description>
            </div>
            {(!firstRun || Boolean(error)) && <Dialog.Close className="icon-button" title="关闭"><X size={18} /></Dialog.Close>}
          </header>

          <div className="stepper" aria-label="配置进度">
            {[["form", "服务信息"], ["detect", "自动检测"], ["models", "确认模型"]].map(([id, label], index) => (
              <div className={`step ${step === id ? "active" : ""} ${["detect", "models"].indexOf(step) >= index - 1 ? "done" : ""}`} key={id}>
                <span>{index + 1}</span>{label}
              </div>
            ))}
          </div>

          {step === "form" && (
            <form className="form-stack" onSubmit={(event) => { event.preventDefault(); void runProbe(); }}>
              <label>服务名称<input autoFocus {...form.register("name")} placeholder="例如：公司开发服务" />
                {form.formState.errors.name && <small className="field-error">{form.formState.errors.name.message}</small>}
              </label>
              <label>Base URL<input {...form.register("baseUrl")} placeholder="https://api.example.com/v1" spellCheck={false} />
                {form.formState.errors.baseUrl && <small className="field-error">{form.formState.errors.baseUrl.message}</small>}
              </label>
              <label>API Key
                <div className="input-action">
                  <input {...form.register("apiKey")} type={showKey ? "text" : "password"} placeholder={keyState === "loading" ? "正在读取已保存的密钥…" : "仅用于目标服务和本机配置"} autoComplete="off" disabled={keyState === "loading"} />
                  <button type="button" className="icon-button" title={showKey ? "隐藏密钥" : "暂时显示密钥"} onClick={() => setShowKey(!showKey)} disabled={keyState === "loading"}>
                    {showKey ? <EyeOff size={17} /> : <Eye size={17} />}
                  </button>
                </div>
                {form.formState.errors.apiKey && <small className="field-error">{form.formState.errors.apiKey.message}</small>}
                {keyState === "loaded" && <small>已从系统凭据库读取，可查看或直接修改；保存后会更新系统凭据。</small>}
                {keyState === "failed" && <small className="field-error">读取已保存密钥失败：{keyError}。请重新输入 API Key。</small>}
              </label>
              <details className="advanced">
                <summary>高级选项</summary>
                <div className="form-grid">
                  <label>协议提示<select {...form.register("protocolHint")}><option value="">自动识别</option><option value="openai">OpenAI-compatible</option><option value="anthropic">Anthropic-compatible</option><option value="gemini">Gemini-compatible</option><option value="azure-openai">Azure OpenAI</option><option value="custom">自定义</option></select></label>
                  <label>超时（秒）<input type="number" {...form.register("timeoutSeconds", { valueAsNumber: true })} /></label>
                </div>
              </details>
              <div className="security-note"><ShieldCheck size={18} /><span>默认启用 TLS 校验；API Key 不会写入日志、导出文件或 URL 查询参数。</span></div>
              {formError && <p className="inline-error" role="alert">{formError}</p>}
              <footer className="dialog-actions"><button className="button primary" type="button" onClick={() => void runProbe()} disabled={startingProbe || Boolean(operation)}>{startingProbe ? <LoaderCircle className="spin" size={17} /> : <Search size={17} />}{startingProbe ? "正在启动检测" : keyState === "loading" && initial?.id ? "使用已保存密钥检测" : "开始自动检测"}</button></footer>
            </form>
          )}

          {step === "detect" && (
            <div className="progress-state" role="status">
              {operation ? <><LoaderCircle className="spin" size={34} /><h3>{operation}</h3><p>正在检查 URL、TLS、身份验证、模型元数据和 Codex 工具兼容性。</p></> : error ? <><div className="status-icon error">!</div><h3>自动检测未完成</h3><p>{error}</p><div className="row-actions"><button className="button" onClick={() => setStep("form")}><ChevronLeft size={17} />返回修改</button><button className="button primary" onClick={() => setStep("models")}>手动选择</button></div></> : null}
            </div>
          )}

          {step === "models" && (
            <div className="result-stack">
              <div className="detect-result"><div className="status-icon success"><Check size={22} /></div><div><h3>{probeResult ? "已识别服务" : "使用手动配置"}</h3><p>{probeResult?.userMessage ?? "自动检测没有返回结果，请确认协议并手动填写模型 ID。"}</p></div><span className="confidence">{probeResult ? `${Math.round(probeResult.confidence * 100)}% 置信度` : "手动"}</span></div>
              <dl className="summary-list"><div><dt>规范化地址</dt><dd>{probeResult?.normalizedBaseUrl ?? form.getValues("baseUrl")}</dd></div><div><dt>协议</dt><dd><span className="protocol-badge">{probeResult?.protocol ?? form.getValues("protocolHint") ?? "custom"}</span></dd></div><div><dt>可用模型</dt><dd>{probeResult?.models.length ?? 0} 个</dd></div></dl>
              {detectedProtocol === "openai" && probeResult?.codexCompatibility && (() => {
                const copy = codexCompatibilityCopy[probeResult.codexCompatibility];
                return <div className={`security-note ${copy.tone === "warning" ? "compat-warning" : ""}`}><ShieldCheck size={18} /><span><strong>{copy.title}</strong><br />{copy.detail}{probeResult.codexProbeModel ? ` 探测模型：${probeResult.codexProbeModel}` : ""}</span></div>;
              })()}
              <div className="model-list" aria-label="可用模型">
                {detectedModels.slice(0, 20).map((model) => <label className="model-row" key={model.id}><input type="radio" name="model" checked={(form.watch("defaultModel") ?? detectedModels[0]?.id) === model.id} onChange={() => selectModel(model.id)} /><span><strong>{model.displayName}</strong><small>{model.id}</small></span><em>{model.contextWindow ? `${model.contextWindow.toLocaleString()} tokens` : model.source === "server" ? "服务端返回" : "规则匹配"}</em></label>)}
                {detectedModels.length > 20 && <p className="model-list-note">列表显示前 20 个模型，默认模型下拉包含全部检测结果。</p>}
                {!probeResult?.models.length && <label className="manual-model">模型 ID<input {...form.register("defaultModel", { required: true })} placeholder="例如：my-coding-model" /></label>}
              </div>
              {detectedModels.length > 0 && <label className="default-model-select">默认模型
                <select value={form.watch("defaultModel") ?? detectedModels[0]?.id ?? ""} onChange={(event) => selectModel(event.target.value)}>
                  {detectedModels.map((model) => <option value={model.id} key={model.id}>{model.displayName}（{model.id}）</option>)}
                </select>
              </label>}
              {activeModelId && (
                <ReasoningTierPicker
                  capability={activeModel?.reasoning}
                  selection={activeSelection}
                  onChange={changeReasoning}
                  onReprobe={initial?.id ? () => void reprobeReasoning() : undefined}
                  reprobing={reprobingReasoning}
                  verifications={savedProvider?.reasoningVerifications}
                  onVerify={initial?.id ? (tier) => void verifyReasoning(tier) : undefined}
                  verifying={verifyingReasoning}
                />
              )}
              {probeResult?.reasoningNote && <small className="reasoning-note">{probeResult.reasoningNote}</small>}
              {detectedProtocol === "anthropic" && (
                <section className="claude-profile" aria-label="Claude Code 映射配置">
                  <div className="form-grid">
                    <label>Claude Code 模型档位
                      <select value={claudeProfile} onChange={(event) => {
                        const profile = event.target.value as ClaudeModelProfile;
                        form.setValue("claudeModelProfile", profile);
                        if (profile === "haiku") form.setValue("claudeExtendedContext", false);
                        const defaultModel = form.getValues("defaultModel");
                        if (defaultModel) form.setValue("claudeModelMappings", assignClaudeMapping(form.getValues("claudeModelMappings") ?? {}, profile, defaultModel));
                      }}>
                        <option value="sonnet">Sonnet（日常编程）</option>
                        <option value="opus">Opus（复杂推理）</option>
                        <option value="haiku">Haiku（快速轻量）</option>
                      </select>
                    </label>
                    <label>上下文窗口
                      <select {...form.register("claudeExtendedContext", { setValueAs: (value) => value === "true" })} disabled={claudeProfile === "haiku"}>
                        <option value="false">标准（200K）</option>
                        <option value="true">扩展（1M）</option>
                      </select>
                    </label>
                  </div>
                  <div className="form-grid claude-mapping-grid">
                    {claudeProfiles.map((profile) => <label key={profile.id}>{profile.label} 映射模型
                      <select aria-label={`${profile.label} 映射模型`} value={claudeMappings[profile.id] ?? ""} onChange={(event) => selectClaudeMapping(profile.id, event.target.value)}>
                        <option value="">使用默认模型</option>
                        {detectedModels.map((model) => <option value={model.id} key={model.id}>{model.displayName}（{model.id}）</option>)}
                      </select>
                    </label>)}
                  </div>
                  <p>程序会把 Claude Code 可识别的 Sonnet、Opus、Haiku 档位路由到对应检测模型，并在 /model 中显示映射后的名称。仅当中转模型真实支持 1M 时选择扩展模式。</p>
                </section>
              )}
              {error && <p className="inline-error">{error}</p>}
              <footer className="dialog-actions split"><button className="button" onClick={() => setStep("form")}><ChevronLeft size={17} />返回</button><button className="button primary" onClick={save} disabled={Boolean(operation)}>{operation ? <LoaderCircle className="spin" size={17} /> : <Check size={17} />}保存服务</button></footer>
            </div>
          )}
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
