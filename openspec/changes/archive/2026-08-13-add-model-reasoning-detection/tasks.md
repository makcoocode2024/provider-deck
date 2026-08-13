## 1. 模块1 · 后端探测接口（改 `model.rs` / `reasoning_selection.rs` / `lib.rs`）

- [x] 1.1 在 `src-tauri/src/model.rs` 新增 `NativeParamKind` 枚举（`unknown` / `effort-enum` / `token-budget` / `boolean-toggle`，`rename_all = "kebab-case"`），实现 `Default` 为 `Unknown`
- [x] 1.2 在 `model.rs` 新增 `MatchedCustomTier`（`tier_id` / `label` / `rule_pattern` / `rule_match_type` / `supported_protocols`），`rename_all = "camelCase"`，全字段 `#[serde(default)]`
- [x] 1.3 在 `model.rs` 新增 `ModelReasoningMeta`（`supported_protocols` / `native_param_kind` / `matched_custom_tiers` / `builtin_tiers_compatible: Option<bool>`），`rename_all = "camelCase"`，全字段 `#[serde(default)]`，附注释写明本结构只投影已有事实、不产生能力结论
- [x] 1.4 在 `model.rs` 新增 `ReasoningDetectionCacheEntry`（`base_url` / `model_id` / `detected_at` / `ttl_seconds` / `native_param_kind` / `builtin_tiers_compatible`）与 `AppSettings.reasoning_detection_cache: Vec<...>`，`#[serde(default)]`；同步更新 `AppSettings::default()`
- [x] 1.5 在 `src-tauri/src/reasoning_selection.rs` 新增 `matching_custom_tiers(model_id, settings) -> Vec<MatchedTier>`：逐条扫规则表、保持表序、跳过空 pattern、跳过引用已删档位的规则、同一档位去重；`match_name_fallback` 与 `resolve_fallback_params` 签名不动
- [x] 1.6 在 `reasoning_selection.rs` 新增 `native_param_kind(capability) -> NativeParamKind` 与 `builtin_tiers_compatible(capability) -> Option<bool>`，映射只来自 `ReasoningControl` 与 `ReasoningSupport`，函数体内不出现任何模型名字面量
- [x] 1.7 在 `src-tauri/src/lib.rs` 新增 `#[tauri::command] detect_model_reasoning(store, provider_id, model_id) -> AppResult<ModelReasoningMeta>`：查不到 Provider 返回 `AppError::ProviderNotFound`，读缓存命中即返回，未命中按能力表投影并回写缓存；注册进 `invoke_handler`
- [x] 1.8 在命令体上写明「本函数不发出站请求」的注释，说明真要重探走 `reprobe_model_reasoning`
- [x] 1.9 单测：多条规则命中同一模型时按表序返回全部；引用已删档位的规则被跳过且不报错；无规则命中返回空 `Vec`
- [x] 1.10 单测：`native_param_kind` 三种 control 各一例 + 能力缺失返回 `Unknown`；`builtin_tiers_compatible` 的 `Some(true)` / `Some(false)` / `None` 各一例
- [x] 1.11 单测：缓存命中不重算、换 base_url 不复用缓存、过期后重算
- [x] 1.12 单测 `detection_cache_carries_no_secrets`：序列化缓存条目后断言不含 `apiKey` / `api_key` / `sk-`
- [x] 1.13 单测：旧 `state.json`（无 `reasoningDetectionCache` 键）加载成功，且自定义档位 / 名称规则 / 逐模型兜底一条不丢
- [x] 1.14 单测：调用本命令前后 `ModelInfo.reasoning` 与 `confidence` 完全不变
- [x] 1.15 闸门：`cargo test --lib --manifest-path src-tauri/Cargo.toml` 全绿，271 例基线无新增失败（实测 282 passed / 0 failed）

## 2. 模块2 · 前端契约与档位卡片重构（改 `types.ts` / `backend.ts` / `reasoning.ts` / `ReasoningTierPicker.tsx`）

- [x] 2.1 在 `src/domain/types.ts` 手写镜像 `NativeParamKind` / `MatchedCustomTier` / `ModelReasoningMeta`，字段名与 Rust 侧 camelCase 序列化结果逐一核对
- [x] 2.2 在 `src/services/backend.ts` 的 `AppBackend` 加 `detectModelReasoning(providerId, modelId)`；`TauriBackend` 走 `invoke("detect_model_reasoning", { providerId, modelId })`（camelCase 参数名）
- [x] 2.3 `BrowserBackend.detectModelReasoning`：按同一套规则从 localStorage settings 算匹配档位，`nativeParamKind` 从 `mockCapabilities` 推；不污染真实路径
- [x] 2.4 在 `src/domain/reasoning.ts` 新增 `tierPickerGroups(capability, meta, settings)`：三段固定顺序（matched-custom → builtin → global-fallback），空分组不产出，不重算生效档位
- [x] 2.5 在 `reasoning.ts` 新增 `writeTargetSummary(...)`：三场景文案，`scene` 由既有 `reasoningOrigin` 映射，`omitted` 返回 `undefined`
- [x] 2.6 在 `src/state/useAppStore.ts` 加 `detectModelReasoning` action 与按 `(providerId, modelId)` 索引的 meta 缓存 + `detecting` 标记；不碰 `models`、不碰 `reasoningVerifications`
- [x] 2.7 `ReasoningTierPicker` 新增 props：`meta?` / `detecting?` / `onCreateTier?()`；组件保持零 store 依赖的受控形态
- [x] 2.8 `ReasoningTierPicker` 渲染探测加载态：`detecting` 期间档位控件禁用；探测失败不清空已有档位与选择、不出现"不支持"字样
- [x] 2.9 用 `tierPickerGroups` 重构档位列表，使其在 `supported` 与非 `supported` 两个分支都生效；`unsupported` 分支不渲染 matched-custom 分组
- [x] 2.10 未探明且无匹配档位时：当前档位显示为全局回退档，档位区域顶部渲染「新建自定义档位」按钮（调 `onCreateTier`）
- [x] 2.11 用 `writeTargetSummary` 替换 `FallbackNotice` 里的冲突文案
- [x] 2.12 在 `ProviderWizard.tsx` 挂载探测调用（进入 / 切换模型时触发）；档位弹窗宿主与 `onCreateTier` 接线留到模块3（`CustomTierDialog` 尚未导出，见模块3 3.1）
- [x] 2.13 vitest：`tierPickerGroups` 三段顺序、空分组不产出、matched-custom 保持后端返回顺序
- [x] 2.14 vitest：`writeTargetSummary` 三场景文案 + `omitted` 返回 `undefined`
- [x] 2.15 vitest：`ReasoningTierPicker` 探测中禁用控件、探测失败保留原有档位、未探明模型出现新建入口、`unsupported` 不出现新建入口
- [x] 2.16 vitest：`backend.test.ts` 断言 `detectModelReasoning` 的 invoke 参数名为 camelCase
- [x] 2.17 闸门：`node_modules/.bin/tsc --noEmit -p tsconfig.app.json` clean、`npm run lint` clean、`npm run test` 全绿（99 → 138）

## 3. 模块3 · 档位编辑器联动刷新（改 `Pages.tsx`，新增 E2E）

- [x] 3.1 `CustomTierDialog` 新增可选入参 `prefillRule?: { pattern; matchType }` 与 `onSaved?(tier)`；不新建第二个弹窗组件（实现为 `onSave(tier, rule?)` 回传规则草稿，宿主负责落盘，避免弹窗自行写 settings）
- [x] 3.2 从模型卡片入口打开弹窗时以当前模型名预填匹配规则，且该预填可被用户改写或删除后再保存（清空 pattern 即视为不建规则，空 pattern 会命中一切模型）
- [x] 3.3 保存流程：落盘 settings（含新档位与新规则）→ 重新调 `detectModelReasoning` → 新档位出现在 `matchedCustomTiers` 时对当前 modelId `onChange` 选中它（选中写入 `reasoningFallbacks` 逐模型兜底，而非名称规则：规则首命中优先，早于它的规则会遮蔽新档位）
- [x] 3.4 新档位不适配当前模型（规则不命中 / 当前协议无参数）时保持原选择，不显示为已生效（判定收敛到 `tierWritableAtEndpoint`，与下拉里「当前端点无可写参数」同一实现）
- [x] 3.5 自动选中只作用于当前 modelId，其他模型的既有选择不得改动（复用既有 `upsertFallback`）
- [x] 3.6 vitest：新建后自动选中；不适配时不选中；其他模型选择不变
- [x] 3.7 E2E 新增用例：从模型卡片新建档位后，下拉自动刷新出该档位并成为选中项
- [x] 3.8 E2E 新增用例：新建档位前后置信度标签不变
- [x] 3.9 闸门：`npx playwright test` 全绿，无新增失败；记录改动前后用例数（基线 62 tests in 2 files → 66，65 passed + 1 skipped；跳过的是 `onboarding.spec.ts:468` 既有的 narrow-only 用例，非新增）
- [x] 3.10 闸门：改了 `e2e/**` 后必须重跑 `npm run lint`（tsc 看不到 e2e）—— clean

## 4. 模块4 · 配置预览文案同步（改 `ConfigPreview.tsx`）

- [x] 4.1 `ConfigPreview` 调用同一个 `writeTargetSummary`，展示三场景说明；不自行拼接文案（进一步共用 `WriteTargetNote` 渲染组件，句子与标记排版一并对齐）
- [x] 4.2 `omitted` 场景不展示兜底提示，改为表明依探测结论省略档位（复用 `originLabel("omitted")`，不新造字符串）
- [x] 4.3 兜底场景文案含「未探测」标注与「仅用于写入配置文件，实时请求不发送推理参数」两句
- [x] 4.4 vitest：同一模型同一时刻，`ConfigPreview` 与 `ReasoningTierPicker` 展示的场景与档位名一致（比对两处渲染文本逐字相等）
- [x] 4.5 vitest：探测失败 / 预览出错的提示文字不含 API 密钥片段
- [x] 4.6 闸门：五道全跑一次（cargo 284 / tsc 0 errors / vitest 154 / lint 0 warnings / playwright 65 passed + 1 既有 skip），全部通过

### 4a. 附加 · 探测缓存脏窗口修复（用户指定并入本模块）

- [x] 4a.1 新增 `reasoning_selection::invalidate_detection_cache(settings, base_url, model_id) -> bool`，按 `(归一化 base_url, model_id)` 精确删，不按 provider 清空
- [x] 4a.2 `reprobe_model_reasoning` 在回写能力的同一次 `store.update` 内作废缓存，归一化规则与写缓存侧一致
- [x] 4a.3 单测：只删目标条目，同 provider 别的模型与别的端点同名模型不受影响，无条目可删返回 `false`
- [x] 4a.4 单测：过期条目也要真的删掉（命中判定看 TTL、作废只看键，这一不对称需被钉住）

## 5. 交付

- [x] 5.1 输出五道闸门结果表格，标注增量用例数与基线（cargo 271 → 284、vitest 99 → 154、playwright 62 → 66；逐模块汇报，汇总见 `HANDOFF.md` 10.6 与 10.4）
- [x] 5.2 输出新增 / 修改文件清单（逐模块汇报，汇总见 `HANDOFF.md` 10.5 与 10.4）
- [x] 5.3 输出 UI 文案修复前后对比（逐模块汇报，规范见 `HANDOFF.md` 10.7）
- [x] 5.4 不创建 git commit，停在待确认状态 —— 已按原约束执行到确认点；随后用户明确授权提交与推送，遂产出 `6b5481f`（模块1–3）与 `690cdb6`（模块4 + 缓存修复）。本条以「已按约束停下并取得授权」计完成
