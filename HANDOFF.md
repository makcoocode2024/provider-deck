# Provider Deck 项目交接文档

> 交接时间：2026-08-11 10:45（Asia/Shanghai）
>
> 交接基线：master / 9a0fd37105000bc511096e3396b44754a4a7d998
>
> 文档范围：本文件只描述 Provider Deck，项目根目录为 G:\provider deck。不要误在 G:\分屏器（另一个 KVM/分屏项目）中修改。

## 1. 项目标识（Project Identity）

### 核心目标

Provider Deck 是一个本地优先（local-first）的跨平台桌面配置中心，帮助 AI 编程工具用户通过图形界面完成第三方 AI Provider 的添加、真实连通性探测、模型获取、服务自测、客户端配置预览/写入、备份与恢复。

它面向不熟悉终端、环境变量和多份 CLI 配置文件的用户。当前重点兼容 Codex CLI、Claude Code 和 OpenCode，并为其他客户端提供安装检测与手动指引。

### 战略意图

- 对标同类 Provider 切换/配置工具的工作流，但不得复制 CC-Switch、Codex++ 或任何竞品的代码、商标、界面、专有素材。
- 对第三方模型网关采用真实 HTTP 接口探测，而非依据模型名、供应商名或黑名单猜测兼容性。
- 尽量让 Codex 保持 Responses 协议。只有 Chat Completions 的后端由内置本地代理适配，不把 CODEX_WIRE_API=chat 作为默认方案。

### 不可变更的硬性业务约束

1. API Key 的主存储必须是系统凭据库：Windows Credential Manager、macOS Keychain、Linux Secret Service。不得写入 Provider 状态 JSON、操作日志、导出文件、诊断输出或 Git。
2. 内置协议代理只可监听 127.0.0.1；不得改为 0.0.0.0、局域网地址或开放 CORS。每个 Provider 必须有独立本地令牌，Codex 配置只能保存该本地令牌，不能保存上游 API Key。
3. 配置写入必须遵守“合并、预览、外部修改校验、备份、原子替换、失败回滚”的链路，默认开启“只生成配置”，禁止全量覆盖用户既有配置。
4. 未知模型的上下文窗口优先从 /v1/models 或 /v1/models/{id} 的元数据读取；不能做高成本长上下文撞限测试，也不能只按名称硬编码窗口。
5. Codex 兼容性必须通过真实 /v1/responses 和（必要时）/v1/chat/completions 的工具请求探测判定，不能靠模型名黑名单。
6. 对 Chat-only 网关必须在 UI 明示 Responses 能力降级及“Provider Deck 退出后，本地代理和相关 Codex 会话会断开”。

## 2. 当前系统状态（Current System State）

### Git 与发布基线

- 当前分支：master
- 工作树：干净，跟踪关系为 master...origin/master。
- 最新提交：9a0fd37105000bc511096e3396b44754a4a7d998。
- 提交时间：2026-08-10 18:11:33 +08:00。
- 提交说明：feat: add chat recovery and adaptive provider reasoning。
- GitHub 私有仓库：makcoocode2024/provider-deck。
- 已发布版本：v0.1.11，为正式 Release（不是草稿或预发布），发布时间 2026-08-10 18:27:34 +08:00。
- Release 附件：安装版 ProviderDeck-Setup-0.1.11-x64.exe、便携版 ProviderDeck-Portable-0.1.11-x64.exe、README.txt、SHA256SUMS.txt。

### 已完好实现

- Provider 新增、编辑、删除、切换当前服务；编辑时可从系统凭据库读取 API Key，支持密码掩码和临时可见。
- OpenAI-compatible、Anthropic-compatible、Gemini-compatible 协议探测和模型列表读取；Azure OpenAI 能被识别，部署名仍需用户手工填写。
- Provider 自测：模型列表连通/鉴权检查，加一次极小真实对话（Reply with OK only.）并返回延迟和回显摘要。
- 通过真实请求判定 Codex 能力：Full、FunctionToolsOnly、ChatProxy、ResponsesUnsupported、Unknown、NotApplicable。
- 本机 Responses → Chat Completions 代理，支持非流式与 SSE 流；含 function/custom 工具转换、namespace 丢弃提示、上游错误透传和进程内 previous_response_id 快照。
- Codex 配置合并、模型目录生成、默认模型/上下文窗口/推理档位写入；可修复历史目录中非法的 apply_patch_tool_type: function。
- Claude Code 配置合并和第三方模型映射：通过 modelOverrides 映射官方 slot，支持扩展上下文及 CLAUDE_CODE_MAX_CONTEXT_TOKENS。
- 写入前哈希校验、备份、原子替换、失败回滚；Unix 收紧文件权限。
- 系统托盘：打开、隐藏、退出；关闭主窗口时隐藏以维持本地代理。
- Codex 会话缓存与加密备份/导入/合并/替换/回滚，使用 XChaCha20-Poly1305；备份密钥保存于系统凭据库。
- build_all.bat：锁定依赖安装、Tauri NSIS 构建、便携 EXE 收集、README 和 SHA-256 生成。

### 已部分完成或需重点复核

- 自适应推理档位只有 Rust 后端完整落地。AppSettings 和 reasoning.rs 已支持自动/手动档位、有效档位和推荐说明；但 src/domain/types.ts 的 AppSettings 未声明这些字段，设置页也没有控制项。前端保存设置会以 serde 默认值回写，可能覆盖已保存的推理状态。
- 本地 Responses 代理是有限协议适配层，不是完整 Responses 服务端。基本对话、标准函数工具、部分 custom 工具和流式文本已实现；高级 Responses 能力无法无损模拟。
- Claude Code 配置逻辑有 Rust 单测，但尚未在多版本真实 Claude Code 和多家第三方网关组合上完成完整人工验收。
- v0.1.11 的安装/便携包已构建并上传，但没有“从 GitHub 下载后在干净 Windows 用户环境安装、启动、卸载”的人工记录。

### 当前损坏/调试状态

- 没有已知的编译失败或未提交源码变更。
- CI 有明确配置错误：.github/workflows/ci.yml 只监听 main，而当前默认/工作分支是 master；向 master 推送不会自动触发该工作流。

## 3. 架构与技术图谱（Architecture & Technical Map）

### 技术栈与主要依赖

- 桌面壳：Tauri 2、Rust 2021、Tauri tray 和 dialog 插件。
- 前端：React 19、TypeScript、Vite、Zustand、React Hook Form、Zod、Radix UI、Lucide。
- 后端 HTTP：Axum、Tokio、Reqwest（rustls TLS/流式）、eventsource-stream、async-stream。
- 配置与安全：toml_edit、serde_json、keyring、directories、sha2、chacha20poly1305。
- 测试：Vitest、Testing Library、Playwright、Rust 单元测试（httpmock、tempfile）。

### 核心模块

| 模块 | 职责 |
| --- | --- |
| src-tauri/src/model.rs | Rust 领域模型和跨 IPC 序列化契约：Provider、ProbeResult、ModelInfo、AppSettings。 |
| src-tauri/src/protocol.rs | URL 规范化、模型读取、服务自测、协议探测、Codex Responses/custom/function 探测、上下文/参数量读取。 |
| src-tauri/src/local_proxy.rs | 仅本地环回端口的 Axum 代理、Provider 路由注册、鉴权、上游请求及 SSE 转换。 |
| src-tauri/src/responses_chat.rs | Responses 与 Chat Completions 双向映射，custom/function/namespace 和流事件适配。 |
| src-tauri/src/config.rs | Codex/Claude/OpenCode 配置合并、模型目录、预览、哈希校验、备份、原子写入与恢复。 |
| src-tauri/src/credentials.rs | 上游 API Key、本地代理 token、聊天备份密钥的系统凭据库读写。 |
| src-tauri/src/chat_store.rs | 进程内对话快照、磁盘缓存、加密备份、导入、回滚。 |
| src-tauri/src/reasoning.rs | 自动推理档位推荐与 NVIDIA 显存信息读取。 |
| src-tauri/src/lib.rs | Tauri command、状态协调、代理生命周期、托盘、关闭窗口行为。 |
| src/services/backend.ts | 前端 IPC 适配器；浏览器后端仅用于测试/预览，真实写入必须经 Tauri。 |
| src/state/useAppStore.ts | Zustand 工作流状态、操作状态和错误状态。 |
| src/components/ProviderWizard.tsx / Pages.tsx | Provider 编辑检测、模型获取、自测、配置与设置 UI。 |

### 关键逻辑流

~~~mermaid
flowchart LR
    UI["React UI"] --> IPC["Tauri IPC commands"]
    IPC --> Probe["protocol.rs：真实探测/模型读取/自测"]
    IPC --> Secret["credentials.rs：系统凭据库"]
    IPC --> State["storage.rs：state.json 原子持久化"]
    Probe --> Provider["Provider + ModelInfo + ProbeResult"]
    Provider --> Config["config.rs：预览、备份、合并写入"]
    Provider --> Proxy["local_proxy.rs：Chat-only Provider 注册"]
    Codex["Codex CLI / Responses wire API"] --> Proxy
    Proxy --> Adapter["responses_chat.rs"]
    Adapter --> Gateway["第三方 /v1/chat/completions"]
    Gateway --> Adapter
    Adapter --> Codex
~~~

### 本地代理的关键契约

1. 监听由系统分配的 127.0.0.1:0 端口，可持久化端口偏好但必须接受端口冲突后的重新绑定。
2. Codex 使用 http://127.0.0.1:<port>/providers/<provider-id>/v1/responses，并携带本地 proxy token。
3. 代理从内存路由取得真实上游 Base URL/API Key，转换后请求上游 /v1/chat/completions；真实 Key 不写入 Codex 配置。
4. Responses custom 工具转换为带 input 字段的 Chat function；namespace 和未知工具类型被丢弃并记录降级提示；标准 function 保持 function。
5. 非流式 Chat choices[0].message 转为 Responses output[]；流式 Chat SSE 转为合成的 response.* SSE 事件。

## 4. 近期工作与决策背景（Recent Work & Key Decisions）

### 最近两小时的实际改动

截至本交接文档生成时，最近两小时没有源码或配置文件改动。当前工作树仍干净。

最近的外部发布操作是：已将 v0.1.11 的 Windows 安装版、便携版、说明和 SHA-256 清单上传并发布到 GitHub Releases。该操作不改变仓库工作树。

### 最近一次代码提交的决策背景

提交 9a0fd37 集中完成以下内容，后续不要把它们“简化回去”：

1. 聊天恢复使用加密备份而不是明文导出。chat_store.rs 用 XChaCha20-Poly1305 对备份负载认证加密，密钥置于系统凭据库；同时支持合并、替换、快照回滚和旧格式读取。
2. 第三方 Responses/Chat 网关按真实能力探测：先探 Responses custom，再探 function；Responses 不可用时才探 Chat function 并标记 ChatProxy。这样可避免第三方网关收到 type: custom/namespace 后出现 unknown variant 400。
3. Codex 继续用 Responses，不把 CODEX_WIRE_API=chat 作为默认方案。Chat-only 后端经本地代理适配，以尽可能保留 Codex 的工作契约。
4. Codex 模型目录中 apply_patch_tool_type 的合法值只能是 freeform 或不写。历史错误值 function 会导致 provider-deck-model-catalog.json 解析失败，启动时只删除该非法旧值。
5. 严格 Chat 网关不接受 Responses 的 xhigh/max 推理档位时，转换为 high；Codex catalog 只生成 minimal、low、medium、high 四档。
6. 未知模型上下文以服务端元数据为准。缺失时保守使用 200k 并标注未验证，不做烧钱的撞限请求。
7. 主窗口关闭时隐藏到托盘不是 DOS 窗口残留或崩溃，其目的是保持本地代理可用；真正退出必须用托盘“退出程序”。

## 5. 已知风险与待解决问题（Known Risks & Blockers）

### 最高优先级

1. 前后端 AppSettings 不一致：Rust 已有自适应推理字段，TypeScript 类型、浏览器测试后端和设置页没有同步；保存设置有状态默认化风险。
2. CI 不随 master 推送运行：工作流写的是 branches: [main]，仓库实际使用 master。必须修复触发分支，并在 GitHub Actions 确认一次。

### 功能和兼容性风险

- Chat-only 代理不能无损模拟 Responses 内置 web/file/code 工具、namespace、MCP、托管文件、服务端 conversation/store、跨进程 previous_response_id、原生 reasoning item/summary、完整 usage/logprobs/事件时序。UI 必须继续显示降级说明。
- previous_response_id 快照只存在于 Provider Deck 进程；应用重启、端口变更或程序退出后，旧会话不能继续依赖它。
- OpenAI-compatible 网关差异大：response_format、tool_choice、SSE chunk、content 数组和 reasoning 字段可能不兼容。必须以真实 Provider 和真实模型做最小自测。
- Claude Code 的第三方 slot 映射会受 CLI 版本和服务商权限影响；配置正确不等于后端允许该模型。需在真实 CLI 中检查 /model 和简单对话。
- 某些客户端固有地需要在其自身配置中保存可读密钥；Provider Deck 的主存储不落盘约束不能自动消除该限制。保持只生成模式、脱敏预览、权限收紧和用户确认。
- 允许自签名证书会降低 TLS 安全性，只能由用户明确选择，不能默认开启。
- “编辑查看 API Key”会经 Tauri IPC 把密钥放入前端内存。不得在 console、错误上报、诊断、剪贴板和崩溃报告泄露。
- BrowserBackend 在浏览器测试模式使用 localStorage/内存模拟密钥；它不是生产安全存储，也不能用于真实 Provider。

### 发布与运维风险

- GitHub Release 资产已上传，但下载数为 0，尚无干净机器下载、安装、启动、卸载的人工记录。
- Windows 包未代码签名，SmartScreen/杀毒软件可能提示；macOS notarization 与 Windows 签名需要独立证书和 CI 密钥流程。
- build_all.bat 仅负责 Windows x64；跨平台包须在对应 OS Tauri 环境或 GitHub Actions 中构建。

## 6. “禁止改动”清单（Do Not Touch List）

除非用户明确提出需求、先写迁移/回滚方案并补齐测试，下一会话不得重构或改变以下安全与兼容性契约：

1. src-tauri/src/credentials.rs 的服务名、provider-id 账号键、代理 token 与聊天备份密钥隔离方式。修改会导致已有用户读不到密钥或备份。
2. src-tauri/src/local_proxy.rs 的 127.0.0.1 绑定、loopback 请求校验、每 Provider 本地 token 验证，以及“不把上游 Key 写入 Codex 配置”的设计。
3. src-tauri/src/config.rs 中的预览哈希校验、备份、atomic_replace、失败回滚、配置合并策略。不得改为全量 JSON/TOML 重写。
4. config.rs 中 Codex catalog 的 apply_patch_tool_type 规则：只能是 freeform 或省略；repair_legacy_codex_catalog 只能删除历史非法的 function，不能删除其他用户字段。
5. src-tauri/src/responses_chat.rs 的 custom → function(input) 映射和反向恢复、工具名去冲突、namespace 显式降级。任何变化必须覆盖流式和非流式测试。
6. src-tauri/src/model.rs 与 src/domain/types.ts 的字段命名/serde camelCase 是持久化状态和 IPC 契约。变更必须同时提供状态迁移和前后端同步修改。
7. src-tauri/src/lib.rs 的“关闭窗口隐藏到托盘”逻辑。在 ChatProxy 使用期间，不能随意改成关闭即退出。
8. 不得重新引入这些旧错误：默认 CODEX_WIRE_API=chat、按模型名黑名单判兼容性、真实 API Key 进入日志/Provider 导出/Git、apply_patch_tool_type 写成 function。

## 7. 明确的下一步计划（Next Steps）

按优先级执行：

1. 修复自适应推理前后端断层：在 src/domain/types.ts 增加四个 Rust 对应字段；在 src/components/Pages.tsx 设置页提供“自动推荐/手动档位”控制和说明；同步 BrowserBackend、Zustand 保存链路、Vitest 与 Playwright。必须验证“保存其他设置不会重置推理设置”。
2. 修复 CI 分支：将 .github/workflows/ci.yml 的 push 分支改为 master（可并列保留 main），推送一个文档/小修复提交确认 Actions 运行。
3. 做真实端到端验收：选择一个原生 Responses 网关和一个仅 Chat Completions 网关，分别测试模型获取、服务自测、Codex 配置、普通对话、function/custom 工具、SSE、托盘隐藏和退出后预期断连。
4. 做真实 Claude Code 验收：测试保存后的 /model、默认 Sonnet/Opus/Haiku slot、[1m] 映射和上下文窗口提示；记录 CLI 版本和行为。
5. 完成回归后再提升版本号，执行 build_all.bat，在干净 Windows 用户环境安装/运行/卸载，再发布新 Release。

## 8. 验证命令（Verification Plan）

在 G:\provider deck 执行。先确认没有真实 Key 被放进 .env、日志或 Git 状态。

~~~powershell
git status --short --branch
git diff --check

npm ci
npm run lint
npm test -- --run
npm run build

cargo test --manifest-path src-tauri\Cargo.toml

npx playwright install chromium
npm run test:e2e
~~~

Windows 发布包验证：

~~~powershell
cmd.exe /d /c build_all.bat
Get-ChildItem release\ProviderDeck-0.1.11-windows-x64
Get-FileHash release\ProviderDeck-0.1.11-windows-x64\*.exe -Algorithm SHA256
Get-Content release\ProviderDeck-0.1.11-windows-x64\SHA256SUMS.txt
~~~

发布前还必须人工完成以下不可自动替代的验证：

1. 使用真实、非生产高权限的第三方 API Key 做最小连通性/对话探测。
2. 在 Codex CLI 和 Claude Code 中分别验证生成后的配置；不把真实 Key 粘进终端日志或问题报告。
3. 在 Windows 上验证关闭窗口后代理仍工作、从托盘“退出程序”后代理确实停止。
4. 下载 GitHub Release 的安装版与便携版，在干净用户环境完成启动、添加测试 Provider、退出和卸载验证。

## 9. Phase D：Runtime Verification（2026-08-12）

> 本节在 Phase D 收尾时追加。前 8 节写于基线 9a0fd37，未随本次改动同步，冲突处以本节为准。
>
> 基线漂移：第 2 节「工作树干净 / HEAD 9a0fd37」、第 5 节最高优先级第 1 条（AppSettings 前后端断层）、
> 第 7 节第 1-2 条（推理断层与 CI 分支）都已完成。当前 HEAD 为 e01db13，master 领先 origin/master
> 5 个提交（bfc0c4b、2045c9d、9623d40、2139e80、e01db13，全部为 Rust 侧推理链路）；Phase D 的前端改动
> 尚未提交，仍在工作树里。

### 9.1 四层边界

Runtime Verification 是「用一次真实响应确认某档位在某端点上确实产出了推理产物」的独立链路。它与 Discovery
共享 `(base_url, model_id)` 这把外键，但两者**不互相写入**。

| 层 | 归属对象 | 写入者 | 绝对不做的事 |
| --- | --- | --- | --- |
| Discovery（能力发现） | `ModelInfo.reasoning: ReasoningCapability` | `reasoning_discovery.rs` | 不读验证历史；不因验证结果改 `confidence` 或 `evidence` |
| Verification（运行时验证） | `Provider.reasoningVerifications: Record<modelId, RuntimeVerification[]>` | `reasoning_verification.rs`、`BrowserBackend.verifyModelReasoning` | 不回写 `ModelInfo.reasoning`；不产出 evidence；不改 confidence |
| UI 展示 | 组件内渲染，无独立状态 | `ReasoningTierPicker.tsx` | 不把 Confirmed 显示成「官方支持」；不把 Rejected/Failed 显示成「不支持推理」 |
| Persistence | `state.json` 里 Provider 节点 | `lib.rs` 的三条 Provider 写入路径 | 不跨 provider、跨端点串数据 |

这条边界是刻意的：Discovery 回答「服务端声称支持什么」，Verification 回答「这个端点此刻真的吐推理产物吗」。
两者结论可以不一致，而不一致本身就是有用的信息。一旦允许验证结果回写能力表，就再也分不清某条结论
是探测来的还是用户点按钮点出来的。

### 9.2 三态语义

`VerificationResult` 是 `#[serde(tag = "status")]` 的三态枚举，三态**全部入库**：

| 状态 | 载荷 | 含义 | UI 文案 | 明确不等于 |
| --- | --- | --- | --- | --- |
| `confirmed` | 无 | 该端点在该档位下确实返回了推理产物 | `已验证 {tierLabel}` | 不等于「官方支持」，不等于 discovery 结论 |
| `rejected` | `reason` | 请求成功，但响应里没有该协议的推理字段 | `此 endpoint 下「{tierLabel}」未检测到推理产物` | **不等于 Unsupported** |
| `failed` | `error` | 请求本身失败（网络、鉴权、限流、上游报错） | `验证失败` + 错误原文 | **不等于 Unsupported** |

Rejected 是有效的负事实，Failed 是排障线索，隐藏任何一个都会让用户反复点同一个按钮却看不到痕迹。

`ReasoningConfidence::Verified` 是既存死档：后端生产代码无一处写入，Runtime Verification 也不用它。
它的 label 是「真实响应证实」，与验证 UI 的三态文案刻意区分开；`src/domain/reasoning.ts:142-153`
的 `confidenceLabels` 上方有守卫注释说明这一点。

### 9.3 数据流

~~~text
用户点击「验证「{tier}」档位」
  ↓
ProviderWizard.tsx          onVerify handler；新建流程没有 provider id，不传 onVerify，整个验证区不渲染
  ↓
useAppStore.ts              verifyReasoning action；前端唯一允许追加 verification 的地方
  ↓
backend.ts                  AppBackend.verifyModelReasoning({ providerId, modelId, tier })，参数 camelCase
  ↓
Rust command                verify_model_reasoning → reasoning_verification.rs 发一次真实请求并判定
BrowserBackend mock         localStorage 后端，按 provider-deck.e2e.verify-result 决定三态
  ↓
reasoningVerifications      追加入库：Rust 走 lib.rs:441，前端走 appendVerification，两端都是 append-only
  ↓
UI history                  ReasoningTierPicker 读 savedProvider.reasoningVerifications 渲染徽章与历史列表
~~~

链路上四个容易踩的点：

1. **invoke 参数必须 camelCase**。Tauri 2 按 camelCase → snake_case 匹配 command 形参，写
   `provider_id` / `model_id` 会直接报参数缺失。
2. **追加语义在两端各实现一次，但都必须是 append-only**。追加逻辑有两处独立实现：
   - Rust command 层：`lib.rs:441` 的 `saved.reasoning_verifications.entry(model_id).or_default().push(verification)`
   - 前端 domain 层：`src/domain/reasoning.ts` 的 `appendVerification()`，被 `useAppStore` 与 `BrowserBackend` 调用

   两者不共享代码，但语义必须保持一致：**old history + new record，绝不覆盖已有 verification history**。
   在各自那一端内部不再重复实现 —— store 层和组件层都不自己拼数组，也不允许组件层直接改
   `reasoningVerifications`；Rust 侧除 `lib.rs:441` 外没有第二个写入点。
   改动任何一端时都要同步检查另一端，否则 Vitest/Playwright 会绿而生产行为不同。
3. **UI 读 store 而不是 initial 快照**。`ProviderWizard.tsx:142-144` 从 `useAppStore` 取
   `savedProvider`，因为 `initial` 是打开向导那一刻的快照，读它看不到刚追加的记录。
   同一处 capability 仍走 `probeResult`（`:379` 的 `capability={activeModel?.reasoning}` 与 `:384` 的
   `verifications={savedProvider?.reasoningVerifications}` 分列），两条数据流的源不变。
4. **剪枝规则与 selection 刻意不对称**。`retain_for_endpoint`（`reasoning_verification.rs:260-272`）
   按 `(base_url, model_id)` 双键剪，换端点即失效；`reasoning_selection::prune_missing` 只按 model_id 剪，
   因为选择是用户意图，换端点后依然成立。

### 9.4 四条不可违反的约束

Phase D 全程遵守，后续也不得放松：

1. 禁止用模型名判断推理能力。
2. 禁止新增硬编码模型清单。
3. 推理能力归属 `(base_url, model_id)`，不是 AppSettings 的全局字段。
4. 旧 `state.json` 必须能加载 —— 靠 `#[serde(default)]`（`model.rs:124` 的 `reasoning_verifications` 就带它）。

另外：`RuntimeVerification` 只存 normalized base_url / model_id / tier / binding / result / verified_at / protocol
七个字段。**禁止保存 API key、请求内容、响应全文。**

### 9.5 文件修改记录

Rust 侧在 Phase D 零改动（`git status --porcelain -- ':/src-tauri'` 为空）；后端能力由已提交的
bfc0c4b…e01db13 五个 commit 提供。下表是 Phase D 的前端改动，全部仍在工作树。

**D1 领域类型（375 insertions, 2 deletions）**

| 文件 | 改动 |
| --- | --- |
| `src/domain/types.ts` | +54/-1：新增 `RuntimeVerification`、`VerificationResult` 三态判别联合、`Provider.reasoningVerifications?` |
| `src/domain/reasoning.ts` | +133：`appendVerification`、`verificationsForTier`、`latestVerification`、`verificationSummary`（三态文案，`:282-299`）等 |
| `src/domain/reasoning.test.ts` | +190/-1：三态序列化、追加语义、外键归属、文案断言 |

**D2 前端管道（416 insertions, 4 deletions）**

| 文件 | 改动 |
| --- | --- |
| `src/services/backend.ts` | +63：`AppBackend.verifyModelReasoning` 接口、`TauriBackend` invoke 实现、`BrowserBackend` mock（`:324-360`） |
| `src/services/backend.test.ts` | +152/-1：invoke 参数 camelCase、mock 三态、入库形状 |
| `src/state/useAppStore.ts` | +30：`verifyReasoning` action，前端唯一的追加入口（Rust 侧的入口是 `lib.rs:441`） |
| `src/state/useAppStore.test.ts` | +175/-3：追加不覆盖、失败不擦除历史、confidence/evidence 不被触碰 |

**D3 UI（576 insertions, 6 deletions）**

| 文件 | 改动 |
| --- | --- |
| `src/components/ReasoningTierPicker.tsx` | +142/-4：验证区、三态徽章、折叠历史列表、费用提示 |
| `src/components/ProviderWizard.tsx` | +18/-2：`savedProvider` 订阅、`onVerify` 门控（无 id 不渲染验证区） |
| `src/styles.css` | +19：`.reasoning-verification-*`、`.verification-badge` |
| `src/components/ReasoningTierPicker.test.tsx` | +215/-1：16 例，含三态文案与「不出现不支持」否定断言 |
| `src/components/ProviderWizard.test.tsx` | 新增 +188：4 例，验证入口门控与数据源分离 |

**D4 E2E（182 insertions）**

| 文件 | 改动 |
| --- | --- |
| `e2e/reasoning-verification.spec.ts` | 新增 +182：6 例 × 2 project = 12，覆盖 confirmed/rejected/failed 三态与 `page.reload()` 后的持久化 |

未被 Phase D 触碰但相关的文件：`src/components/Pages.tsx`、`src/test/setup.ts`、`vite.config.ts`、
`e2e/onboarding.spec.ts`（均由 2045c9d 修改，Phase D 未再动）。

### 9.6 测试结果

2026-08-12 于 `G:\provider deck` 实测，全绿：

| 命令 | 结果 |
| --- | --- |
| `cargo test --lib --manifest-path src-tauri/Cargo.toml` | 200 passed; 0 failed; 0 ignored（2 warnings，见 9.7） |
| `npx tsc --noEmit -p tsconfig.app.json` | 0 errors |
| `npx vitest run` | 66 passed（6 files：url 5 / reasoning 20 / useAppStore 13 / backend 8 / ReasoningTierPicker 16 / ProviderWizard 4） |
| `npx playwright test` | 47 passed, 1 skipped（skip 是 `onboarding.spec.ts:332` 的窄屏专用例，在 desktop project 下自跳，与 Phase D 无关） |
| `npm run lint`（`eslint . --max-warnings 0`） | 0 warnings |

**tsconfig 覆盖盲区**：`tsconfig.app.json` 只 `include: ["src"]`，`tsconfig.node.json` 只 include
`vite.config.ts` / `playwright.config.ts` / `eslint.config.js`。**`e2e/**` 不被任何 tsconfig 覆盖**，
`tsc` 不检查它。但 eslint 覆盖 `**/*.{ts,tsx}` 且 CI 用 `--max-warnings 0` —— 改 e2e 文件后必须跑
`npm run lint`，它才是 e2e 在 CI 里的实际闸门。

### 9.7 已知问题

**1. `BrowserBackend.saveProvider` 不 carry `reasoningVerifications`（未处理，记为独立 issue）**

- Rust（`lib.rs:94-101`）在 saveProvider 时 carry 已有历史，再用 `retain_for_endpoint` 按端点剪枝。
- `BrowserBackend.saveProvider`（`backend.ts:201-222`）从零构造 Provider，不 carry 这个字段。
- 暂不处理的理由：当前 Runtime Verification 流程不经过 saveProvider，E2E 也不依赖它；修它会扩大 Phase D 范围。
- 触发条件：如果将来有「先验证、再保存 Provider」的流程，浏览器测试后端会静默丢历史，而真实后端不会 ——
  届时测试会通过但行为与生产不一致。修的时候要连带补一条 backend 层测试。

**2. Rust `dead_code` warnings（未处理，不影响编译与测试）**

`cargo test --lib` 输出两条：

- `reasoning_capability.rs:84` `EvidenceSource::label` 从未被调用
- `reasoning_capability.rs:401` `ReasoningCapability::tier_by_id` 从未被调用

两者都是给 UI 预留的展示/查询辅助，实际展示逻辑落在了 TypeScript 侧（`src/domain/reasoning.ts`
自己有一套 label 表）。保留现有 commit hash，不为 warning 改历史提交。

**已消解**：早前记录的「`model.rs:1` unused import」现已不存在。该 import 由 `2045c9d` 引入时确实无人使用，
`e01db13` 加上 `reasoning_verifications: HashMap<String, Vec<RuntimeVerification>>`（`model.rs:124`）后
自然消掉，cargo 当前不再报它。无需改历史 commit。

### 9.8 禁止的 shortcut

下面每一条都会让 9.1 的边界塌掉，任何后续会话都不得引入：

1. `Confirmed` → `confidence: Verified`。
2. verification 结果 → `reasoning.evidence`。
3. verification 结果 → `ModelInfo.reasoning`（任何字段）。
4. 用 verification history 替代或覆盖 discovery 结果。
5. 把 capability 与 verification history 合并成一个数据结构或一次查询。
6. `history = [verification]`（覆盖式写入）；只能 `old + new`。这条对两端同时生效 ——
   Rust 的 `lib.rs:441` 与前端的 `appendVerification()` 是两处独立实现，任一处退化成覆盖式都算违反。
7. 为了让测试通过而改生产逻辑、改 `src-tauri/**`、改 Runtime Verification 数据模型。



## 10. Phase E：模型推理档位自动探测与档位联动 UI（2026-08-13）

OpenSpec 变更 ID：`add-model-reasoning-detection`，规格与任务清单在
`openspec/changes/add-model-reasoning-detection/`。本阶段分四个模块串行开发，每个模块闸门全绿后
由用户确认再推进下一个。截至本次提交，模块 1、2、3 已完成并确认，模块 4 尚未开始。

### 10.1 要解决的问题

改造前的档位 UI 有四个缺口：未探测的模型只能回落到全局 high 兜底；没有把「模型名匹配规则命中了哪些
自定义档位」呈现出来；未知模型没有就地新建自定义档位的入口；实时请求链路与配置写入链路的提示文案
互相冲突，用户无法判断填的档位到底会不会发出去。

### 10.2 新增的核心抽象

| 名称 | 位置 | 职责 |
| --- | --- | --- |
| `detect_model_reasoning(providerId, modelId)` | `src-tauri/src/lib.rs` | 只投影本地已有事实，不发出站请求。真要重探走 `reprobe_model_reasoning` |
| `ModelReasoningMeta` | `model.rs` / `types.ts` | 端点可写协议、原生参数形态、命中的自定义档位、内置档位兼容性 |
| `NativeParamKind` | `model.rs` / `types.ts` | `unknown` / `effort-enum` / `token-budget` / `boolean-toggle` |
| `matching_custom_tiers` | `reasoning_selection.rs` | 按规则表序返回命中档位，跳过空 pattern 与悬空档位引用 |
| `tierPickerGroups` | `src/domain/reasoning.ts` | 三段固定顺序分组：匹配到的自定义档位 → 内置档位 → 全局回退档 |
| `writeTargetSummary` | `src/domain/reasoning.ts` | 写入场景文案的唯一来源，兜底结算仍委托既有 `fallbackNotice` |
| `tierWritableAtEndpoint` / `autoSelectableTier` | `src/domain/reasoning.ts` | 「该档位在当前端点写不写得出参数」的唯一实现，下拉徽章与自动选中共用 |

`builtinTiersCompatible` 是三值的：`true` / `false` / `null`（无法确认）。用 `false` 表示未知会把
「不知道」伪装成「不兼容」，禁止这样简化。

### 10.3 三条实现偏差（与 tasks.md 原文不同，已就地注记）

1. 弹窗回调定为 `onSave(tier, rule?)` 而非 `onSaved(tier)`：`CustomTierDialog` 只回传规则草稿，
   落盘由宿主负责，弹窗保持零 store 依赖，设置页与向导共用同一个组件。
2. 自动选中写 `reasoningFallbacks`（逐模型兜底）而不是名称规则：规则是首命中优先，排在前面的旧规则
   会遮蔽刚建的档位；逐模型兜底是唯一能保证该模型确实用上新档位的表达。
3. 预填规则被用户清空时不建规则：空 pattern 会命中一切模型，存下去比不存危险。

### 10.4 模块 4：配置预览文案同步与缓存脏窗口修复（已完成）

`ConfigPreview` 不再自己拼推理文案。它与 `ReasoningTierPicker` 共用**同一个渲染组件**
`WriteTargetNote`（从 `ReasoningTierPicker.tsx` 导出），入参来自同一个 `writeTargetSummary`。
共用组件而不是各自调函数再各自渲染，是因为后者只能保证句子相同、保不住标记与排版相同。

`writeTargetSummary` 返回 `undefined` 有两种成因，预览必须区别对待：

- `omitted`（探测已排除写档位）—— 显示 `originLabel("omitted")`，即「不写入档位（依探测结论省略）」。
  不显示任何兜底提示，也不显示档位名；此时报一个档位名是错的。
- 已探明但用户钉死了显式 binding —— 整块不出声，那由向导的「高级」区自己说明。

预览是只读视图，`ReasoningWriteNote` 只读 store 里已有的 `reasoningMeta` 投影，**不触发探测**：
打开预览不该产生任何请求。meta 缺失也不影响文案，写入场景由能力表与设置表结算。

#### 缓存脏窗口修复

**改动点**：新增 `reasoning_selection::invalidate_detection_cache(settings, base_url, model_id) -> bool`
（`reasoning_selection.rs`，紧接 `upsert_detection_cache` 之后），并在
`reprobe_model_reasoning`（`lib.rs`）回写能力的**同一次 `store.update`** 里调用它。base_url 走
`protocol::normalize_base_url`，与写缓存那侧同一套归一化规则。

**为什么必须修**：缓存存的是上一次能力结论的投影（`native_param_kind` /
`builtin_tiers_compatible`）。重探换掉了 `ModelInfo.reasoning`，却把旧条目留着，
`detect_model_reasoning` 会在 TTL 剩余时间里继续返回旧投影。TTL 用的是
`reasoning_capability::TTL_UNKNOWN_SECONDS`（6 小时），所以脏窗口最长 6 小时。

**复现步骤（修复前）**：
1. 添加一个 Provider，默认模型是未探明模型（能力为 `unknown`，投影 `nativeParamKind: "unknown"`）。
2. 打开编辑向导进到「确认模型」步 —— `detect_model_reasoning` 落一条缓存，TTL 6 小时。
3. 点「重新探测」，且这次真的探到了能力（例如 `effortEnum`）。
4. 档位区的参数形态说明仍是重探前的结论 —— `ParamKindNote` 不出现，或仍显示旧形态。
   6 小时内反复重探都不变；等到 TTL 过期才会自动纠正。

**作用域**：按 `(归一化 base_url, model_id)` 精确删，不按 provider 清空。同一 provider 下别的模型、
同名模型在别的端点，缓存都原样留着 —— 连带清掉等于让那些模型白白重算一遍。

**性能影响**：一次 `Vec::retain`，条目数等于用户打开过档位区的
`(端点, 模型)` 组合数，量级是几十条，成本可忽略。它在既有的 `store.update` 事务内执行，
不新增落盘次数。反过来说，作废后下一次 `detect_model_reasoning` 必然走重算分支，
但那只是一次本地遍历，无出站请求。

**一个刻意的不对称**（有测试钉住）：`detection_cache_hit` 过滤过期条目，
`invalidate_detection_cache` 只看键、不看 TTL。所以作废会把过期条目也一并删掉。
若将来有人给作废也加上 TTL 过滤，过期条目会永久留在 `state.json` 里越积越多。

#### 模块 4 文件改动

| 文件 | 改动 |
| --- | --- |
| `src/components/ConfigPreview.tsx` | +40/-2：新增 `ReasoningWriteNote`，复用 `WriteTargetNote` 与 `writeTargetSummary`；`omitted` 走 `originLabel` |
| `src/components/ReasoningTierPicker.tsx` | 1 行：`WriteTargetNote` 由私有改为 `export` |
| `src-tauri/src/reasoning_selection.rs` | +21 实现 / +40 测试：`invalidate_detection_cache` 与两条单测 |
| `src-tauri/src/lib.rs` | +5：`reprobe_model_reasoning` 在回写能力的同一次 update 里作废缓存 |
| `src/components/ConfigPreview.test.tsx` | 新增：5 例（见下） |
| `src/styles.css` | +2：`.preview-reasoning-note` |
| `.gitignore` | +7：本地工具链永久过滤项 |

#### 模块 4 新增测试用例

Rust（2 例，`reasoning_selection.rs`）：

| 用例 | 场景 |
| --- | --- |
| `invalidate_detection_cache_removes_only_the_target_entry` | 只删目标键；同 provider 别的模型、别的端点同名模型都留着；无条目可删时返回 `false` 且不 panic |
| `invalidate_detection_cache_also_drops_expired_entries` | 过期条目虽不命中但仍在数组里，作废要真的删掉，防止 `state.json` 里堆积 |

vitest（5 例，`ConfigPreview.test.tsx`）：

| 用例 | 场景 |
| --- | --- |
| 兜底场景带「未探测」标注与配置文件专属说明 | 4.3：两句话都在 |
| `omitted` 场景不显示兜底提示，改为说明依探测结论省略 | 4.2：正向断言「不写入档位（依探测结论省略）」，反向断言「配置写入」「全局回退档」整句缺席 |
| 没有默认模型时整块说明缺席，diff 仍照常渲染 | 边界：不因缺模型而崩，也不空渲染一行 |
| 与 `ReasoningTierPicker` 展示同一个场景与同一个档位名 | 4.4：比对两个组件**渲染出的文本**逐字相等，不是各自再调一次函数——后者只能证明函数是纯的 |
| 预览文案不含任何密钥或鉴权字段片段 | 4.5：扫全 DOM 文本，禁 `sk-` / `AIza` / `apiKey` / `api_key` / `Authorization` / `Bearer`；同时正向断言兜底说明确已渲染，避免"什么都没渲染"导致否定断言空转通过 |

新增用例全部不含明文密钥与鉴权字段，错误文案取「写入失败：目标文件被占用（已回滚）」这类脱敏措辞。

#### 模块 4 闸门结果

2026-08-13 五道全跑：

| 命令 | 模块 3 后 | 模块 4 后 | 结果 |
| --- | --- | --- | --- |
| `cargo test --lib --manifest-path src-tauri/Cargo.toml` | 282 | 284 | 全绿 |
| `node_modules/.bin/tsc --noEmit -p tsconfig.app.json` | — | — | 0 errors |
| `npm run test`（vitest） | 149 | 154 | 全绿 / 7 files |
| `npm run lint` | — | — | 0 warnings |
| `npx playwright test` | 66 | 66 | 65 passed, 1 skipped（既有 narrow-only skip） |

模块 4 未新增 E2E：文案一致性由 4.4 的跨组件断言覆盖，且该断言比 E2E 更强——
它逐字比对两个组件的渲染文本，E2E 只能分别断言各自出现了某段文字。

### 10.9 下一轮：本次变更已可归档

模块 1–4 全部完成，`openspec/changes/add-model-reasoning-detection/tasks.md` 全项勾选。
下一轮可走 `openspec-archive-change`，把三份 spec 合入 `openspec/specs/`。归档前无未决项。

### 10.5 文件修改记录

Rust 侧改动集中在模块 1；模块 2、3 为纯前端。整阶段 17 个文件，2121 insertions / 62 deletions。

| 文件 | 模块 | 改动 |
| --- | --- | --- |
| `src-tauri/src/model.rs` | 1 | +206：`NativeParamKind`、`MatchedCustomTier`、`ModelReasoningMeta`、`ReasoningDetectionCacheEntry` 与 `AppSettings.reasoning_detection_cache` |
| `src-tauri/src/reasoning_selection.rs` | 1 | +355：`matching_custom_tiers`、`native_param_kind`、`builtin_tiers_compatible` 及单测 |
| `src-tauri/src/lib.rs` | 1 | +95：`detect_model_reasoning` 命令与注册 |
| `src/domain/types.ts` | 2 | +47：三个类型的手写镜像 |
| `src/services/backend.ts` | 2 | +83：接口、`TauriBackend` invoke、`BrowserBackend` 派生 |
| `src/state/useAppStore.ts` | 2 | +47：`detectModelReasoning` action、`reasoningMeta` / `detectingReasoning` |
| `src/domain/reasoning.ts` | 2 / 3 | +217：分组、写入文案、可写性判定 |
| `src/components/ReasoningTierPicker.tsx` | 2 | +137/-16：分组渲染、写入说明、参数形态说明；删掉旧 `FallbackNotice` |
| `src/components/Pages.tsx` | 3 | +46：导出 `CustomTierDialog` 与 `TierRuleDraft`，加 `prefillRule` 与规则输入 |
| `src/components/ProviderWizard.tsx` | 2 / 3 | +107：探测触发、弹窗宿主、落盘后重探与条件自动选中 |
| `src/styles.css` | 2 / 3 | +17：分组与 `.reasoning-badge.discovered` |
| 测试：`reasoning.test.ts` +187、`ReasoningTierPicker.test.tsx` +176、`ProviderWizard.test.tsx` +192、`backend.test.ts` +95、`useAppStore.test.ts` +94、`e2e/reasoning-verification.spec.ts` +82 | 1–3 | 见 10.6 |

`AppSettings`（TypeScript 侧）刻意不镜像 `reasoningDetectionCache`：那是后端的私有缓存，前端不读不写。

### 10.6 测试结果

2026-08-13 于 `G:\provider deck` 实测：

| 命令 | 基线 | 本阶段 | 结果 |
| --- | --- | --- | --- |
| `cargo test --lib --manifest-path src-tauri/Cargo.toml` | 271 | 282 | 全绿（模块 1 实测；模块 2、3 零 Rust 改动，未重跑） |
| `node_modules/.bin/tsc --noEmit -p tsconfig.app.json` | — | — | 0 errors |
| `npm run test`（vitest） | 99 | 149 | 全绿 / 6 files |
| `npm run lint`（`eslint . --max-warnings 0`） | — | — | 0 warnings |
| `npx playwright test` | 62 | 66 | 65 passed, 1 skipped |

E2E 那条 skipped 是 `e2e/onboarding.spec.ts:468` 既有的 `test.skip(project !== "narrow-chromium")`，
在 desktop project 下自跳，与本阶段无关。新增 2 个用例 × 2 project = 4，故 62 → 66。

9.6 记的 tsconfig 盲区依然成立：**`e2e/**` 不被任何 tsconfig 覆盖**，改过 e2e 之后必须重跑
`npm run lint`，它是 e2e 在 CI 里的唯一闸门。

### 10.7 文案规范（新增约束，后续不得放松）

设定性取值与探测结论必须在措辞上可区分：

- 自定义档位、全局回退档这类**用户设定**的取值，绝不使用「支持 / 兼容 / 已确认 / 已验证 / 已探明」。
  兜底场景说「全局回退档（未探测，可新建自定义档位适配此模型）」，命中自定义档位说
  「命中你设定的档位（未探测）」。
- 只有 discovery 真的探到能力时，才允许出现「已探明档位」。
- 所有写入说明恒附一句「仅用于写入配置文件，实时请求不发送推理参数。」——
  这对应 9.x 一直守着的边界：`resolve_binding` 实时链路的推理参数保持 Omitted。
- 档位在当前端点写不出参数时，行内显示「当前端点无可写参数」，不隐藏该档位。隐藏会让用户以为保存失败。

### 10.8 禁止的 shortcut（Phase E 追加）

9.8 各条继续有效，另加四条：

1. 改 `resolve_binding` 的实时请求逻辑，或让实时链路自动填推理参数。
2. 让 `detect_model_reasoning` 发出站请求，或让它写 `ModelInfo.reasoning`、`confidence`、
   `reasoningVerifications` 中任何一处。
3. Anthropic / Gemini 协议在没有自定义档位时编造推理数值 —— 必须回落全局兜底。
4. 用模型名推断能力，或为新增结构体省掉 `#[serde(default)]`（旧 `state.json` 必须无损加载）。

---

## 会话启动摘要（Session Resume Prompt）

~~~text
你正在维护 Provider Deck，项目根目录是 G:\provider deck，不是 G:\分屏器。
基线：master，HEAD 9a0fd37105000bc511096e3396b44754a4a7d998，工作树应保持干净；v0.1.11 已发布。

先阅读 HANDOFF.md。最高优先级是：
1) 同步 Rust AppSettings 与 src/domain/types.ts / SettingsPage：把 auto_reasoning_mode、manual_reasoning_level、effective_reasoning_level、reasoning_match_message 接入 UI、BrowserBackend、测试，确保保存普通设置不会重置推理模式；
2) 修复 .github/workflows/ci.yml：当前只触发 main，但仓库用 master。

硬约束：API Key 主存储只用系统凭据库，不能进入 Provider JSON/日志/导出/Git；本地协议代理只能监听 127.0.0.1，不能把上游 Key 写入 Codex 配置；真实能力必须接口探测，不能按模型名硬编码；配置写入必须合并+预览+哈希校验+备份+原子替换；Codex catalog 的 apply_patch_tool_type 只能是 freeform 或省略，绝不能写 function。

修改后至少运行：npm run lint；npm test -- --run；cargo test --manifest-path src-tauri\Cargo.toml。涉及 UI 再运行 npm run test:e2e；发布前运行 build_all.bat。
~~~
