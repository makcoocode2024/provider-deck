## Why

当前模型推理档位的界面逻辑与实际写入逻辑割裂：未探明能力的模型只能落到全局回退档，界面上没有任何入口告诉用户「这个模型可以用哪个自定义档位适配」，也没有从模型卡片直接新建档位的路径；同一件事（这个模型配置里会写什么档位）在模型卡片、兜底提示、配置预览三处用不同措辞表达，用户无法判断哪句话是探测结论、哪句话是自己的设定。

后端其实已经具备全部事实来源（`reasoning_discovery` 的三级发现结果、`reasoning_selection::resolve_fallback_params` 的兜底三级、`AppSettings` 的自定义档位与名称规则），缺的是一个把这些结论汇总成「这个模型现在能选什么」的读接口，以及一套围绕它的界面交互。

## What Changes

- **新增** Tauri 命令 `detect_model_reasoning`：按 `(provider_id, model_id)` 汇总该模型的推理探测结论与可适配档位，返回 `ModelReasoningMeta`。它是**只读投影 + 按需触发既有发现流程**，不引入任何新的能力判定规则。
- **新增** `ModelReasoningMeta` 结构体（`supported_protocols` / `native_param_kind` / `matched_custom_tiers` / `builtin_tiers_compatible`），全部字段 `#[serde(default)]`。
- **修改** 推理档位卡片交互：进入或切换模型时自动发起探测并显示 loading；档位下拉按「匹配的自定义档位 → 内置档位 → 全局回退档」排序；未探明且无匹配时默认停在全局回退档，并在顶部给出「新建自定义档位」入口，点击后预填当前模型名作为匹配规则。
- **修改** 自定义档位编辑器：保存后自动重新探测并选中刚建好的档位，用户不需要手动重选。
- **修改** 文案：模型卡片与配置预览统一成三种场景的同一套说明（命中自定义匹配档位 / 选中内置档位 / 回落全局兜底），消除现有的措辞冲突。
- **不变**：`resolve_binding` 的实时请求链路仍然 `Omitted`，推理参数只在配置写出层生效；探测不产生 `Verified`；不新增任何按模型名推断能力的逻辑。

### 与原始需求的两处偏差（需确认）

1. **命令签名**：需求写的是 `detect_model_reasoning(model_id)`，实际需要 `(provider_id, model_id)`。推理能力归属 `(base_url, model_id)`，只给 model_id 无法定位端点，也无法复用既有缓存。
2. **文件路径**：需求提到的 `src/components/model/ModelCard.tsx`、`src/components/reasoning/TierEditor.tsx`、`src/components/config/ConfigPreview.tsx` 三个路径在本仓库不存在。对应的真实位置是 `src/components/ReasoningTierPicker.tsx`、`src/components/Pages.tsx`（`CustomTierEditor` / `CustomTierDialog`）、`src/components/ConfigPreview.tsx`。本变更改这三处真实文件，不新建目录（`src/pages` 这类"按惯例新建目录"在本项目是明确禁止的）。

## Capabilities

### New Capabilities

- `reasoning-detection`: 按 `(provider_id, model_id)` 汇总推理探测结论与可适配档位的只读接口，含时效缓存与「无匹配返回空列表」的语义。
- `reasoning-tier-ui`: 推理档位选择界面的探测加载态、三段式档位下拉排序、未知模型的新建档位入口、档位保存后的联动刷新与自动选中。
- `reasoning-copy-consistency`: 档位来源说明在模型卡片与配置预览两处的统一措辞，三种场景一一对应。

### Modified Capabilities

（无。`openspec/specs/` 当前为空，本变更引入的三个能力都是首次成文。既有行为约束以代码注释和 `PROJECT_CONTEXT.md` 为准，本变更不改动它们。）

## Impact

**后端**

- `src-tauri/src/model.rs`：新增 `ModelReasoningMeta`、`MatchedCustomTier`、`NativeParamKind`；`AppSettings` 新增探测缓存字段（全部 `#[serde(default)]`）。
- `src-tauri/src/reasoning_selection.rs`：新增「筛选全部适配当前模型的自定义档位」的查询函数（既有 `match_name_fallback` 只返回首条命中，不够用）。
- `src-tauri/src/lib.rs`：新增 1 个 `#[tauri::command]`，命令总数 28 → 29。
- `src-tauri/src/app_settings.rs`：需求点名了此文件，但**本仓库不存在**该文件，`AppSettings` 住在 `model.rs`。缓存字段落在 `model.rs`。

**前端**

- `src/domain/types.ts`：新增 `ModelReasoningMeta` 等镜像类型（手写镜像，无自动生成）。
- `src/services/backend.ts`：`AppBackend` 新增 `detectModelReasoning`，`TauriBackend` 与 `BrowserBackend` 各实现一份。
- `src/domain/reasoning.ts`：新增档位下拉分组与三场景文案的纯投影函数。
- `src/components/ReasoningTierPicker.tsx`：探测加载态、下拉重构、新建档位入口。
- `src/components/Pages.tsx`：`CustomTierEditor` / `CustomTierDialog` 支持预填模型名规则与保存后回调。
- `src/components/ConfigPreview.tsx`：三场景说明文案。
- `src/state/useAppStore.ts`：新增探测 action 与 meta 缓存。

**测试**

- Rust 单测：当前基线 271 例（实测 `cargo test --lib` 全绿）。
- Vitest：当前 6 个文件。
- Playwright：当前 62 tests in 2 files（`--list` 实测；两个视口）。
- 四道闸门命令不变；E2E 仍不在 CI 与 `build_all.bat` 里。

**不受影响**

实时请求链路、Codex 环境变量鉴权、四大客户端模块、`SupportLevel`、Discovery/Verification 边界、`config.rs` 六步写入管线。
