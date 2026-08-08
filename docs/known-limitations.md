# 已知限制

- Gemini CLI 处于产品迁移期，仅提供实验性生成与手动指引。
- VS Code、Cursor、Windsurf、Cline、Roo Code 和 Continue 不自动写入。
- JSONC 注释保持、YAML 与 dotenv 结构化写入尚未实现。
- Azure OpenAI 需要用户提供部署名与 API Version。
- 系统安全凭据库不可用时不会降级为明文状态存储。
- Linux Secret Service 依赖桌面会话中的 D-Bus 与密钥环服务。
- 不执行普通对话质量测试或长上下文撞限测试；Codex 工具兼容性探测最多发送两次 1-token 请求，可能产生极少量费用。
- Chat-only 服务通过本机 Responses 兼容桥接入 Codex 时，namespace、MCP、Responses 内置工具、文件输入、原生 reasoning 状态和跨进程 previous_response_id 无法无损模拟；Provider Deck 必须保持运行。
- 兼容桥的流式 usage、logprobs 和事件时序取决于 Chat 后端提供的 chunk，部分数据可能为空或被合成。
