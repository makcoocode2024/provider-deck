## Context

动机见 `proposal.md` 的 Why；行为契约见 `specs/` 下三份 spec。这里只写怎么实现。

约束现状（读代码得到，全部不可绕过）：

- 推理能力归属 `(归一化 base_url, model_id)`，落在 `ModelInfo.reasoning`，由 `reasoning_discovery` 独家写入，三档 TTL 在 `reasoning_capability.rs:5-7`。
- 用户意图分三处：`AppSettings.reasoning_fallbacks`（逐模型兜底）、`AppSettings.custom_reasoning_tiers`（自定义档位）、`AppSettings.reasoning_name_rules`（名称规则）。三者都在 `AppSettings` 而非 `Provider`，因为「这个模型该用什么档」跟着模型走、不跟端点走。
- 兜底优先级已有唯一实现：`reasoning_selection::resolve_fallback_params`（单模型兜底 > 名称规则），全局回退档由 `config.rs::codex_reasoning` 的 `legacy_kept()` 兜住。前端有一份等价投影 `reasoning.ts::reasoningOrigin` / `effectiveFallbackTier` / `fallbackNotice`，五种 origin 与后端分支一一对应。
- `match_name_fallback` 只返回首条命中；`resolve_fallback_params` 内部虽然会逐条往下试，但只吐出最终生效的那一条。界面要展示「全部适配档位」，两者都不够用。
- `src-tauri/src/app_settings.rs` 不存在，`AppSettings` 在 `model.rs:435`。需求点名的三个前端路径（`components/model/`、`components/reasoning/`、`components/config/ConfigPreview.tsx`）中只有第三个的文件名对得上，目录结构不对。
- `ReasoningTierPicker` 当前是「五态提前 return」结构：非 `supported` 直接渲染空态 + `FallbackNotice` 就返回，档位列表只在 `supported` 分支出现。新增的「三段式下拉」必须同时活在两个分支里。
- 实测基线：cargo 271 passed，playwright 62 tests in 2 files。

## Goals / Non-Goals

**Goals**

- 一次查询取齐界面所需：探测结论 + 全部适配自定义档位 + 内置档位可用性判断。
- 兜底优先级的结算逻辑仍然只有后端一处权威；前端新增的排序不重算优先级，只做展示分组。
- 探测缓存不新增第二套 TTL 机制。
- 旧 `state.json` 无损加载。

**Non-Goals**

- 不改 `resolve_binding`，实时请求链路一个字不动。
- 不为「内置档位是否兼容」发明新的判定规则：它只能由已有的 `capability.control` 推出，推不出就是「无法确认」。
- 不把 `reasoning_verifications` 或 `ModelInfo.reasoning` 纳入本命令的写路径——本命令对能力表只读。
- 不新建 `src/components/model/`、`src/components/reasoning/`、`src/components/config/` 目录。

## Decisions

### D1 · 命令签名用 `(provider_id, model_id)`，不是 `(model_id)`

需求写的是 `detect_model_reasoning(model_id: String)`。单靠 model_id 无法定位端点，而能力、缓存、探测请求全部按 `(base_url, model_id)` 索引；同一个 `gpt-4o` 在三个中转站下是三条不同的事实。

替代方案是「取当前 Provider」，被否：`ReasoningTierPicker` 在向导里渲染的是**正在编辑的草稿 Provider**，未必是 `is_current` 那一个，取当前会给出另一个端点的结论。

结论：`detect_model_reasoning(provider_id: String, model_id: String) -> AppResult<ModelReasoningMeta>`。

### D2 · 本命令只读能力表，探测触发复用既有命令

需求说"轻量探测模型原生推理参数类型"。项目里已经有一条完整的探测链路（`reprobe_model_reasoning` → `reasoning_discovery::discover_*`，含 Tier 0/1/2 与 TTL 退避）。再写一条"轻量探测"就是第二套发现逻辑，两套结论会打架，而且新的那套必然更弱（不写 evidence、不走 TTL）。

决定：`detect_model_reasoning` **不发出站请求**。它读 `ModelInfo.reasoning`，把结论投影成 `ModelReasoningMeta`。需要真的重探时用户走既有的「重新探测」按钮（`reprobe_model_reasoning`），前端在其成功后再调一次本命令刷新。

代价：本命令无法把「从未探测过」变成「已探测」。这正确——发现是发现的职责。

这同时让需求里的「探测结果增加时效缓存」变成一个更小的东西，见 D3。

### D3 · 缓存只缓存「匹配结算」，不缓存能力

能力已经有三档 TTL 缓存（`reasoning_capability.rs`），再加一层就是两套过期时间。本命令唯一值得缓存的是"规则表 × 模型名"的匹配结果，而那是纯内存计算，比缓存读写还便宜。

决定：`AppSettings` 新增 `reasoning_detection_cache: Vec<ReasoningDetectionCacheEntry>`（`#[serde(default)]`），只存 `(base_url, model_id, detected_at, ttl_seconds, native_param_kind, builtin_tiers_compatible)` 这几项探测投影，**不存**匹配档位列表（它随用户改规则立刻失效，缓存它必然读到脏数据）。

命中判定：`detected_at + ttl_seconds` 未过期且 `(归一化 base_url, model_id)` 一致。过期或不存在时按当前能力表重算并回写。规则表变化不影响这份缓存——它不含匹配结果。

字段 MUST NOT 含密钥、请求体、响应体（沿用 `verification_record_carries_no_secrets` 的同类约束，新增一条同形单测）。

替代方案「不做缓存」也可行且更简单，但需求明确要求缓存，且缓存里存的这几项在跨会话展示时能少一次能力表遍历。保留。

### D4 · 新增 `matching_custom_tiers`，与 `match_name_fallback` 并存

`match_name_fallback` 返回首条命中，语义是「配置写出用哪条」，被 `config.rs` 依赖，不能改签名。

新增：

```rust
pub fn matching_custom_tiers<'a>(
    model_id: &str,
    settings: &'a AppSettings,
) -> Vec<MatchedTier<'a>>   // { rule: &ReasoningNameRule, tier: &CustomReasoningTier }
```

逐条扫规则表，跳过空 pattern、跳过引用已删档位的规则、保持表序。同一档位被多条规则命中时只保留首次出现（否则下拉里出现两个同名项，用户无法区分）。

`resolve_fallback_params` 不动——生效档位仍由它决定。新函数只回答「还有哪些能选」。

### D5 · `NativeParamKind` 从 `capability.control` 推，不从模型名推

```rust
pub enum NativeParamKind { Unknown, EffortEnum, TokenBudget, BooleanToggle }
```

映射来源只有 `ReasoningControl`：`EffortEnum{..} → EffortEnum`、`TokenBudget{..} → TokenBudget`、`BooleanToggle → BooleanToggle`、能力缺失或 `support != Supported` → `Unknown`。

需求提到的 `thinkingBudget / budget_tokens / effort` 是三个协议的**字段名**，而字段名已经由 `reasoning_adapters` 各自持有。在本结构里再存一份字符串字段名等于把适配器知识复制到 model.rs，协议新增时两处都要改。存参数**类别**、由适配器管字段名。

`builtin_tiers_compatible: Option<bool>`：`Some(true)` 仅当 `control` 是 `EffortEnum` 且 `capability.tiers` 非空（内置五档就是映射到 effort 词表的）；`Some(false)` 当探到 `Unsupported`；其余一律 `None`（无法确认）。三态用 `Option<bool>` 而不是 bool——bool 会把「不知道」写成「不兼容」。

### D6 · 前端下拉分组做成纯投影函数

新增 `reasoning.ts::tierPickerGroups(capability, meta, settings)`，返回

```ts
{ kind: "matched-custom" | "builtin" | "global-fallback"; label: string; items: TierPickerItem[] }[]
```

分组顺序固定：matched-custom → builtin → global-fallback。空分组不产出（spec 要求不出现空标题）。

放 domain 而不是组件内：三段式排序会被 `ReasoningTierPicker` 的 supported / 非 supported 两个分支同时用到，写在组件里必然复制一遍。domain 层可被 vitest 零成本直测。

**不在这里重算优先级**：当前生效档位仍由既有 `activeTier` / `fallbackNotice` 给出，分组函数只负责"有哪些可选项、怎么排"。

### D7 · 场景文案收敛到一个函数

新增 `reasoning.ts::writeTargetSummary(capability, meta, settings, modelId)`，返回三场景之一：

| scene | 文案 |
| --- | --- |
| `matched-custom` | `配置写入：{档位名} · 模型名称规则匹配` |
| `builtin` | `配置写入：{档位名} · 已探明档位` |
| `global-fallback` | `配置写入：{全局档} · 全局回退档（未探测，可自定义档位适配此模型）` |

`ReasoningTierPicker` 与 `ConfigPreview` 都调它，两处不各自拼字符串——这是 spec「两处措辞必须一致」唯一可验证的实现方式。

`scene` 由既有 `reasoningOrigin` 映射而来（`model-fallback`/`name-rule` → `matched-custom`，`discovered` → `builtin`，`global-fallback` → `global-fallback`，`omitted` → 不产出），不新增一套判定。

`omitted` 时返回 `undefined`：探测已排除写档位，此时展示任何"配置写入 X"都是错的。

### D8 · 新建档位入口复用 `Pages.tsx` 里的 `CustomTierDialog`

`CustomTierDialog` 已经在设置页可用。给它加两个可选入参：`prefillRule?: { pattern: string; matchType: NameMatchType }` 与 `onSaved?(tier)`。不新建第二个档位弹窗。

`ReasoningTierPicker` 自己不持有弹窗状态——它是受控组件，弹窗由上层（`ProviderWizard`）挂载，`ReasoningTierPicker` 只多一个 `onCreateTier?()` 回调。理由：`ReasoningTierPicker` 目前零 store 依赖、纯 props，加弹窗会把它变成读 store 的组件，破坏它可被 16 个 vitest 用例直测的现状。

保存后的联动：上层收到 `onSaved` → 调 `updateSettings` 落盘 → 重新 `detectModelReasoning` → 若新档位出现在 `matchedCustomTiers` 里则 `onChange` 选中它；不在则不动选择（spec 明确要求）。

### D9 · `BrowserBackend` 也实现 `detectModelReasoning`

E2E 全走 `BrowserBackend`。假实现按同一套规则从 localStorage 里的 settings 算匹配档位，`nativeParamKind` 从 `mockCapabilities` 推。不加"测试专用分支"到真实路径。

## Risks / Trade-offs

| 风险 | 缓解 |
| --- | --- |
| 新命令被误当成"探测入口"，后续有人往里加出站请求，形成第二套发现逻辑 | 命令体内写明"本函数不发出站请求"的注释，并加单测：给一个能力缺失的模型调用本命令，断言返回 `Unknown` 且 `httpmock` 上零请求 |
| `AppSettings` 新增缓存字段，旧 `state.json` 加载失败 | `#[serde(default)]` + 沿用 `legacy_settings_with_removed_reasoning_fields_still_load` 的同形单测，断言旧文件加载后自定义档位/规则/兜底一条不丢 |
| 前端分组函数与后端优先级各算一遍，日后漂移 | 分组函数**不判定生效档位**，生效档位仍只由 `fallbackNotice` / `activeTier` 给出；vitest 断言"分组里的选中项与 `fallbackNotice` 的档位一致" |
| 自动选中把用户在别的模型上的选择改掉 | 自动选中只在 `matchedCustomTiers` 含新档位时对**当前 modelId** 调 `onChange`；spec 有对应场景，vitest + E2E 各一条 |
| 三段式下拉在 `unsupported` 态出现，让用户以为能覆盖探测结论 | `omitted` 场景不产出文案；`unsupported` 分支不渲染 matched-custom 分组（沿用现有"用户设定推翻不了事实"的规则） |
| 探测缓存字段里意外写进端点凭据 | 新增单测 `detection_cache_carries_no_secrets`，序列化后断言不含 `apiKey` / `api_key` / `sk-` |
| E2E 62 tests 基数变动被误判成回归 | 新增用例只加不改；交付时同时给出改动前后数字 |

## Migration Plan

无数据迁移。新增字段全部 `#[serde(default)]`，旧文件加载后视为无缓存。

回滚：本变更只新增命令与新增字段，删除新增代码即回到当前行为；已写入 `state.json` 的缓存字段在旧版本下被静默忽略（`AppSettings` 无 `deny_unknown_fields`）。

分四个模块串行落地，每个模块自带闸门，顺序见 `tasks.md`。

## Open Questions

- `reasoning_detection_cache` 的 TTL 取值。倾向复用 `reasoning_capability.rs` 已有的三档常量而不新增第四个数字；但由于本命令只读能力表、不发请求，这份缓存的实际收益只是省一次遍历，取值不影响任何行为契约。实现时取 `unknown` 档同值，若用户另有偏好可直接改常量。
