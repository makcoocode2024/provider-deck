# Provider Deck

Provider Deck 是一个本地优先的 AI Provider 配置、探测与切换桌面工具。它面向不熟悉终端、环境变量和配置文件的用户，使用 Tauri 2、React、TypeScript 与 Rust 构建。

当前版本完成了最小可用闭环：添加 Provider -> 协议与工具兼容性探测 -> 读取模型 -> 检测客户端 -> 预览配置 -> 备份 -> 原子写入 -> 恢复。应用默认启用“只生成配置”模式，不会在用户确认前修改客户端配置。

对于只实现 Chat Completions 的第三方服务，Provider Deck 可启动仅监听 `127.0.0.1` 的嵌入式兼容桥。Codex 继续使用 Responses wire API，本地桥负责双向转换；该模式不是无损 Responses 实现，使用期间必须保持 Provider Deck 运行。

## 安全边界

- API Key 优先存储在 Windows Credential Manager、macOS Keychain 或 Linux Secret Service。
- OpenAI-compatible 网络探测除模型元数据外，会向最终选定模型发送最多两次、每次最多输出 1 token 的 Responses 工具兼容性请求；界面会在执行前明确说明。
- 默认校验 TLS；自签名证书支持必须由用户显式开启。
- 日志、错误信息、诊断和非敏感导出不包含 API Key。
- 自动写入前检查文件哈希、创建时间戳备份，并通过同目录临时文件原子替换。
- 测试只使用浏览器隔离存储、mock 数据或临时目录，不修改开发机真实配置。
- Codex CLI、Claude Code 等客户端自身可能要求把密钥写入配置文件。Provider Deck 会在预览中说明风险，且必须关闭“只生成配置”后才能执行。
- 本地兼容桥为每个 Provider 使用独立本地令牌；Codex 配置不保存桥接服务的真实上游 API Key。

## 支持矩阵

| 客户端 | 安装检测 | 配置预览 | 自动写入 | 状态 | 说明 |
| --- | --- | --- | --- | --- | --- |
| OpenAI Codex CLI | 是 | 是 | 是 | 已验证结构 | 使用 `~/.codex/config.toml` 的 `model_providers` |
| Claude Code | 是 | 是 | 是 | 已验证结构 | 合并 `~/.claude/settings.json` 的 `env`，保留未知字段 |
| OpenCode | 是 | 是 | 是 | 已验证结构 | 使用公开的 `provider`、`options.baseURL`、`options.apiKey` 结构 |
| Gemini CLI | 是 | 是 | 否 | 实验性 | 官方站点已公告 2026 年产品迁移安排，只生成指引 |
| VS Code | 是 | 手动 | 否 | 仅手动引导 | 不修改扩展内部数据库 |
| Cursor | 是 | 手动 | 否 | 仅手动引导 | 不修改内部数据库 |
| Windsurf | 是 | 手动 | 否 | 仅手动引导 | 不修改内部数据库 |
| Cline / Roo Code / Continue | 有限 | 手动 | 否 | 仅手动引导 | 通过各扩展设置页面配置 |

协议探测支持 OpenAI-compatible、Anthropic-compatible、Gemini-compatible 和 Azure OpenAI。Azure 部署名无法通过通用低权限接口可靠枚举，因此需要手动输入。

“已验证结构”表示配置字段已通过公开文档与本机实际字段核对，不表示当前环境已在目标客户端中执行真实写入。

## 开发

前置环境：

- Node.js 20 或更高版本
- Rust stable 与 Cargo
- Tauri 2 对应平台依赖：Windows WebView2/MSVC、macOS Xcode Command Line Tools、Linux WebKitGTK 及系统库

```bash
npm install
npm run dev
npm run test
npm run lint
npm run test:e2e
npm run build
npm run tauri dev
```

浏览器开发模式不会执行真实网络探测或文件写入；真实功能必须通过 `npm run tauri dev` 启动。Playwright 使用 `--mode test` 的隔离测试后端。

## 打包

```bash
npm run tauri build
```

- Windows：在 Windows 10/11 + MSVC 环境构建 MSI/NSIS。
- macOS：分别使用 Intel 和 Apple Silicon runner 构建；通用包需要在 macOS 上组合目标架构。
- Linux：在 Ubuntu runner 安装 WebKitGTK、AppIndicator、SVG 和构建依赖后构建 deb/AppImage。

GitHub Actions 配置见 `.github/workflows/ci.yml`，会先执行前端检查，再在对应操作系统构建 Tauri 安装包。签名、Apple notarization 和 Windows 代码签名需要仓库自行配置密钥。

## 数据位置

Provider 元数据和备份目录由系统标准应用目录确定：

- Windows：`%APPDATA%\ProviderDeck\Provider Deck` 及系统凭据管理器
- macOS：`~/Library/Application Support/...` 及 Keychain
- Linux：XDG config/data 目录及 Secret Service

真实 API Key 不进入 Provider 状态 JSON。非敏感导出固定排除凭据；当前版本不提供“包含密钥导出”。

## 文档

- [协议适配器开发](docs/protocol-adapters.md)
- [客户端适配器开发](docs/client-adapters.md)
- [Codex 第三方网关兼容性](docs/codex-gateway-compatibility.md)
- [Codex Responses 本地兼容桥](docs/local-responses-chat-proxy.md)
- [安全设计](docs/security.md)
- [已知限制](docs/known-limitations.md)

## 已知限制

- Chat-only 后端无法无损提供 Responses 内置工具、namespace/MCP、文件输入、原生 reasoning 状态和服务端持久会话。
- JSONC 注释保留尚未实现；遇到 JSONC 时应保持“只生成配置”并人工合并。
- YAML、dotenv 配置写入接口尚未开放给首批自动适配器。
- 客户端配置变更通常需要重启对应 CLI 或编辑器。
- Codex 工具兼容性探测可能产生极少量模型调用费用；不会执行普通对话质量测试或长上下文撞限测试。
