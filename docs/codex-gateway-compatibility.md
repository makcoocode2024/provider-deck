# Codex 第三方网关兼容性

## 根因

Codex CLI 当前使用 Responses API。Responses 的 `tools` 是带 `type` 的联合类型，除标准 `function` 外还可能包含 OpenAI 内置工具、MCP 和自由格式 `custom` 工具。Codex 的自由格式补丁能力会产生 `type: "custom"`。

Chat Completions 的自定义工具结构则是 `type: "function"` 加嵌套的 `function` 对象。很多“OpenAI-compatible”网关只实现了 Chat Completions，或只把 Responses 的 `function`、`web_search_preview`、`code_interpreter`、`mcp` 加入反序列化枚举，没有实现 `custom`。这类网关能列出模型，甚至能完成普通 Responses 文本请求，但会在解析 Codex 的工具数组时直接返回 HTTP 400。

当前官方 Codex 配置参考明确规定 `model_providers.<id>.wire_api` 只有 `responses` 一个受支持值。`CODEX_WIRE_API=chat`、`wire_api="chat"`、`enableBuiltinTools` 和 `config.json` 不是当前公开配置接口，不能作为通用降级方案。

官方资料：

- [Codex Configuration Reference](https://learn.chatgpt.com/docs/config-file/config-reference)
- [Responses: Create a model response](https://developers.openai.com/api/reference/resources/responses/methods/create)

## 自动探测

Provider Deck 不按模型名称或供应商名称判断兼容性。读取 `GET /v1/models` 后，使用用户最终选中的模型执行以下探测：

1. 向 `POST /v1/responses` 发送一个最多输出 1 token 的 `custom` 工具请求。
2. 成功：标记为 `full`，Codex 模型目录使用自由格式补丁工具。
3. 404、405 或 501：标记为 `responses-unsupported`，阻止写入无效 Codex 配置。
4. 400 或 422：再发送同等大小的 `function` 工具请求。
5. `function` 成功：标记为 `function-tools-only`，模型目录不写 `apply_patch_tool_type`，让 Codex 使用默认兼容工具。
6. 超时、429 或 5xx：标记为 `unknown`，同样省略自由格式补丁声明并采用保守模式。
7. 保存时如果用户改选了另一个模型，会针对最终模型重新探测，避免用模型 A 的结果配置模型 B。

探测可能产生最多两次极小模型请求，因此可能产生极少量计费。API Key 只放在 Authorization 请求头，不进入 URL、日志、状态文件或错误摘要。

## 最小请求

先测试 Responses 与 custom 工具：

```http
POST {base_url}/responses
Authorization: Bearer $API_KEY
Content-Type: application/json

{
  "model": "selected-model-id",
  "input": "Provider Deck compatibility probe. Reply with OK.",
  "max_output_tokens": 1,
  "tool_choice": "none",
  "tools": [{
    "type": "custom",
    "name": "provider_deck_probe",
    "description": "Compatibility probe; never call this tool.",
    "format": { "type": "text" }
  }]
}
```

如果 custom 请求返回 schema 400，再测试标准 function：

```json
{
  "model": "selected-model-id",
  "input": "Provider Deck compatibility probe. Reply with OK.",
  "max_output_tokens": 1,
  "tool_choice": "none",
  "tools": [{
    "type": "function",
    "name": "provider_deck_probe",
    "description": "Compatibility probe; never call this function.",
    "parameters": {
      "type": "object",
      "properties": {},
      "additionalProperties": false
    }
  }]
}
```

这里的 `{base_url}` 应是 Codex Provider 的 API 前缀。例如用户输入 `https://gateway.example.com/v1`，请求地址是 `https://gateway.example.com/v1/responses`，不能重复追加 `/v1`。

## Codex 配置

当前 Codex 使用 TOML，不是 `config.json`。持久配置位于：

- Windows：`%USERPROFILE%\.codex\config.toml`
- macOS：`$HOME/.codex/config.toml`
- Linux：`$HOME/.codex/config.toml`
- 设置 `CODEX_HOME` 时：`$CODEX_HOME/config.toml`

Provider 配置必须写入用户级文件；项目级 `.codex/config.toml` 不能覆盖 `model_provider` 和 `model_providers`。Provider Deck 使用 TOML 解析器合并字段，保留不归本程序管理的配置，并在写入前备份和校验文件哈希。

```toml
model_provider = "my-gateway"
model = "selected-model-id"
model_context_window = 256000

[model_providers.my-gateway]
name = "My Gateway"
base_url = "https://gateway.example.com/v1"
wire_api = "responses"
requires_openai_auth = false
env_key = "MY_GATEWAY_API_KEY"
```

推荐把密钥临时注入启动进程，而不是持久写入配置：

```powershell
$env:MY_GATEWAY_API_KEY = "..."
codex -c 'model_provider="my-gateway"' -c 'model="selected-model-id"'
```

```cmd
set MY_GATEWAY_API_KEY=... && codex -c model_provider="my-gateway" -c model="selected-model-id"
```

```bash
MY_GATEWAY_API_KEY='...' codex -c 'model_provider="my-gateway"' -c 'model="selected-model-id"'
```

环境变量只存在于该终端及其子进程，关闭终端后失效。持久方案只合并 `config.toml`；Provider Deck 当前不会修改系统级环境变量。

## 降级影响

`apply_patch_tool_type` 当前只接受 `freeform`；写成 `function` 会让 Codex 在启动阶段拒绝解析整个模型目录。`function-tools-only` 因此会省略该字段，使 Codex 不再声明 `type: "custom"` 的自由格式补丁工具，并退回当前版本提供的默认兼容工具。用户可能观察到：

- 大型或复杂补丁需要拆成更多步骤；
- 工具参数占用更多 token；
- 某些依赖自由格式输入的新工具不可用；
- 文本问答、读取文件和普通 shell 操作仍可使用；可用工具的具体集合取决于 Codex CLI 版本。

面向新手的 UI 文案：

> 这个服务可以运行 Codex，但不支持一种高级工具格式。Provider Deck 已自动切换到兼容模式；日常编程仍可使用，复杂修改可能多执行几步。

如果状态为 `responses-unsupported`：

> 这个服务只能处理传统聊天请求，无法运行当前版本的 Codex 编程工具。Provider Deck 没有写入配置，以免启动后持续报错。请更换支持 Responses API 的服务。

## 模型上下文

模型列表和 `GET /v1/models/{model_id}` 会读取以下常见字段：`context_window`、`context_length`、`max_context_length`、`max_input_tokens`、`inputTokenLimit`，以及常见的 `limits`、`metadata`、`capabilities` 嵌套形式。不会根据模型名称猜测窗口，也不会通过发送超长上下文逐级撞限额。

- Codex：把可信值写入 `model_context_window` 和 Provider Deck 模型目录。
- Claude Code：继续使用官方模型 ID 到第三方 ID 的 `modelOverrides`；获得可信窗口值时写入 `CLAUDE_CODE_MAX_CONTEXT_TOKENS`。
- 元数据缺失：保持 200K 安全回退，不自动设置 `CLAUDE_CODE_DISABLE_UNKNOWN_MODEL_WINDOW_ENFORCEMENT=1`。关闭保护可能让未知网关在长会话中突然失败，只应由高级用户明确选择。
