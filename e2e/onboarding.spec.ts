import { expect, test } from "@playwright/test";

test.beforeEach(async ({ page }) => {
  await page.goto("/", { waitUntil: "domcontentloaded" });
  await page.evaluate(() => localStorage.clear());
  await page.reload({ waitUntil: "domcontentloaded" });
});

async function openSettings(page: import("@playwright/test").Page, testInfo: import("@playwright/test").TestInfo) {
  await page.evaluate(() => localStorage.setItem("provider-deck.e2e.providers", JSON.stringify([{
    id: "settings-test-provider",
    name: "设置测试服务",
    baseUrl: "https://settings.example.test",
    protocol: "openai",
    enabled: true,
    isCurrent: true,
    defaultModel: "test-coder",
    models: [],
    connectionState: "connected",
    appliedClients: [],
  }])));
  await page.reload({ waitUntil: "domcontentloaded" });
  if (testInfo.project.name === "narrow-chromium") await page.getByRole("button", { name: "打开导航" }).click();
  await page.getByRole("button", { name: "设置" }).click();
}

async function openProviders(page: import("@playwright/test").Page, testInfo: import("@playwright/test").TestInfo) {
  if (testInfo.project.name === "narrow-chromium") {
    await page.getByRole("button", { name: "打开导航" }).click();
  }
  await page.getByRole("button", { name: "Provider" }).click();
}

test("首次配置完成探测并进入客户端选择", async ({ page }) => {
  await expect(page.getByRole("heading", { name: "添加第一个 AI 服务" })).toBeVisible();
  await page.getByLabel("服务名称").fill("测试开发服务");
  await page.getByLabel("Base URL").fill("api.example.test/v1/");
  await page.getByLabel("API Key").fill("sk-test-only-not-a-secret");
  await page.getByRole("button", { name: "开始自动检测" }).click();
  await expect(page.getByRole("heading", { name: "已识别服务" })).toBeVisible();
  await expect(page.getByText("Codex 本地兼容桥已启用")).toBeVisible();
  await expect(page.getByText("https://api.example.test/v1")).toBeVisible();
  await page.getByRole("button", { name: "保存服务" }).click();
  await expect(page.getByRole("heading", { name: "客户端" })).toBeVisible();
  await expect(page.getByText("测试开发服务")).toBeVisible();
  await expect(page.getByText("已验证").first()).toBeVisible();
});

test("自动探测失败时可返回手动协议选择", async ({ page }) => {
  await page.getByLabel("服务名称").fill("手动服务");
  await page.getByLabel("Base URL").fill("https://manual.example.test");
  await page.getByLabel("API Key").fill("test-key-value");
  await page.getByText("高级选项").click();
  await page.getByLabel("协议提示").selectOption("anthropic");
  await page.getByRole("button", { name: "开始自动检测" }).click();
  await expect(page.getByText("ANTHROPIC", { exact: false })).toBeVisible();
});

test("首次保存失败后仍可关闭配置窗口", async ({ page }) => {
  await page.evaluate(() => localStorage.setItem("provider-deck.e2e.fail-save", "1"));
  await page.getByLabel("服务名称").fill("保存失败服务");
  await page.getByLabel("Base URL").fill("https://save-failure.example.test");
  await page.getByLabel("API Key").fill("test-key-value");
  await page.getByRole("button", { name: "开始自动检测" }).click();
  await expect(page.getByRole("heading", { name: "已识别服务" })).toBeVisible();
  await page.getByRole("button", { name: "保存服务" }).click();
  const dialog = page.getByRole("dialog");
  await expect(dialog.getByText("测试后端模拟保存失败", { exact: true })).toBeVisible();
  await expect(dialog.getByRole("button", { name: "关闭" })).toBeVisible();
  await dialog.getByRole("button", { name: "关闭" }).click();
  await expect(page.getByRole("dialog")).toBeHidden();
});

test("服务保存后重新检测会复用已保存凭据", async ({ page }, testInfo) => {
  await page.getByLabel("服务名称").fill("重新检测服务");
  await page.getByLabel("Base URL").fill("https://reprobe.example.test/v1");
  await page.getByLabel("API Key").fill("saved-test-key");
  await page.getByRole("button", { name: "开始自动检测" }).click();
  await expect(page.getByRole("heading", { name: "已识别服务" })).toBeVisible();
  await page.getByRole("button", { name: "保存服务" }).click();
  if (testInfo.project.name === "narrow-chromium") {
    await page.getByRole("button", { name: "打开导航" }).click();
  }
  await page.getByRole("button", { name: "Provider" }).click();
  await page.getByRole("button", { name: "重新检测" }).click();
  await expect(page.getByText("正在使用已保存的凭据重新检测…")).toBeHidden();
  await expect(page.getByRole("alert")).toHaveCount(0);
  await expect(page.getByText(/1 个模型 · 检测于/)).toBeVisible();
});

test("编辑服务可查看修改密钥并重新自动检测", async ({ page }, testInfo) => {
  const originalKey = "saved-edit-test-key";
  const updatedKey = "updated-edit-test-key";

  await page.getByLabel("服务名称").fill("密钥编辑服务");
  await page.getByLabel("Base URL").fill("https://edit-key.example.test/v1");
  await page.getByLabel("API Key").fill(originalKey);
  await page.getByRole("button", { name: "开始自动检测" }).click();
  await expect(page.getByRole("heading", { name: "已识别服务" })).toBeVisible();
  await page.getByRole("button", { name: "保存服务" }).click();

  await openProviders(page, testInfo);
  await page.getByRole("button", { name: "编辑" }).click();
  const apiKey = page.getByLabel("API Key");
  await expect(apiKey).toHaveValue(originalKey);
  await expect(apiKey).toHaveAttribute("type", "password");
  await page.getByRole("button", { name: "暂时显示密钥" }).click();
  await expect(apiKey).toHaveAttribute("type", "text");
  await apiKey.fill(updatedKey);
  await page.getByRole("button", { name: "开始自动检测" }).click();
  await expect(page.getByRole("heading", { name: "已识别服务" })).toBeVisible();
  await page.getByRole("button", { name: "保存服务" }).click();

  await openProviders(page, testInfo);
  await page.getByRole("button", { name: "编辑" }).click();
  await expect(page.getByLabel("API Key")).toHaveValue(updatedKey);
});

test("已保存密钥读取失败后可手动输入并检测", async ({ page }, testInfo) => {
  await page.evaluate(() => localStorage.setItem("provider-deck.e2e.providers", JSON.stringify([{
    id: "missing-secret-provider",
    name: "缺失凭据服务",
    baseUrl: "https://missing-secret.example.test/v1",
    protocol: "openai",
    enabled: true,
    isCurrent: true,
    defaultModel: "test-coder",
    models: [],
    connectionState: "connected",
    appliedClients: [],
  }])));
  await page.reload({ waitUntil: "domcontentloaded" });
  await openProviders(page, testInfo);
  await page.getByRole("button", { name: "编辑" }).click();
  await expect(page.getByText(/读取已保存密钥失败/)).toBeVisible();
  const apiKey = page.getByLabel("API Key");
  await expect(apiKey).toBeEnabled();
  await apiKey.fill("replacement-test-key");
  await page.getByRole("button", { name: "开始自动检测" }).click();
  await expect(page.getByRole("heading", { name: "已识别服务" })).toBeVisible();
});

test("密钥仍在读取时点击自动检测也会立即开始", async ({ page }, testInfo) => {
  await page.evaluate(() => {
    localStorage.setItem("provider-deck.e2e.secret-delay-ms", "1000");
    localStorage.setItem("provider-deck.e2e.providers", JSON.stringify([{
      id: "slow-secret-provider",
      name: "慢速凭据服务",
      baseUrl: "https://slow-secret.example.test/v1",
      protocol: "openai",
      enabled: true,
      isCurrent: true,
      defaultModel: "test-coder",
      models: [],
      connectionState: "connected",
      appliedClients: [],
      lastCheckedAt: new Date().toISOString(),
    }]));
    localStorage.setItem("provider-deck.e2e.secret.slow-secret-provider", "saved-during-load-key");
  });
  await page.reload({ waitUntil: "domcontentloaded" });
  await openProviders(page, testInfo);
  await page.getByRole("button", { name: "编辑" }).click();
  const detectButton = page.getByRole("button", { name: "使用已保存密钥检测" });
  await expect(detectButton).toBeVisible();
  await detectButton.click();
  await expect(page.getByRole("heading", { name: "已识别服务" })).toBeVisible();
});

test("编辑包含 Rust null 可选字段的服务可以正常检测", async ({ page }, testInfo) => {
  await page.evaluate(() => {
    localStorage.setItem("provider-deck.e2e.providers", JSON.stringify([{
      id: "null-fields-provider",
      name: "https://api.deepseek.com",
      baseUrl: "https://api.deepseek.com",
      protocol: "openai",
      enabled: true,
      isCurrent: true,
      defaultModel: null,
      claudeModelProfile: null,
      claudeExtendedContext: false,
      claudeModelMappings: { sonnet: null, opus: null, haiku: null },
      codexProbeModel: null,
      codexProbeDetail: null,
      models: [],
      connectionState: "connected",
      confidence: null,
      lastCheckedAt: null,
      appliedClients: [],
      errorSummary: null,
    }]));
    localStorage.setItem("provider-deck.e2e.secret.null-fields-provider", "saved-null-fields-key");
  });
  await page.reload({ waitUntil: "domcontentloaded" });
  await openProviders(page, testInfo);
  await page.getByRole("button", { name: "编辑" }).click();
  await expect(page.getByLabel("API Key")).toHaveValue("saved-null-fields-key");
  await page.getByRole("button", { name: "开始自动检测" }).click();
  await expect(page.getByText(/请检查以下信息/)).toHaveCount(0);
  await expect(page.getByRole("heading", { name: "已识别服务" })).toBeVisible();
});

test("Anthropic 中转可配置 Claude Code 模型档位和 1M 上下文", async ({ page }) => {
  await page.getByLabel("服务名称").fill("中转模型服务");
  await page.getByLabel("Base URL").fill("https://relay.example.test");
  await page.getByLabel("API Key").fill("test-key-value");
  await page.getByText("高级选项").click();
  await page.getByLabel("协议提示").selectOption("anthropic");
  await page.getByRole("button", { name: "开始自动检测" }).click();
  const defaultModel = page.getByRole("combobox", { name: "默认模型", exact: true });
  await expect(defaultModel).toBeVisible();
  await defaultModel.selectOption("agnes-2.0-pro");
  const profile = page.getByRole("combobox", { name: "Claude Code 模型档位", exact: true });
  await expect(profile).toBeVisible();
  await profile.selectOption("opus");
  await expect(page.getByRole("combobox", { name: "Sonnet 映射模型", exact: true })).toHaveValue("agnes-2.0-flash");
  await expect(page.getByRole("combobox", { name: "Opus 映射模型", exact: true })).toHaveValue("agnes-2.0-pro");
  await expect(page.getByRole("combobox", { name: "Haiku 映射模型", exact: true })).toHaveValue("agnes-2.0-lite");
  await page.getByRole("combobox", { name: "上下文窗口" }).selectOption("true");
  await page.getByRole("button", { name: "保存服务" }).click();
  await expect(page.getByRole("heading", { name: "客户端" })).toBeVisible();
});

test("保存设置后显示成功反馈", async ({ page }, testInfo) => {
  await openSettings(page, testInfo);
  await page.getByRole("button", { name: "保存设置" }).click();
  await expect(page.getByRole("status")).toHaveText("设置已保存");
});

test("已有服务可以获取最新模型列表", async ({ page }, testInfo) => {
  await page.evaluate(() => {
    localStorage.setItem("provider-deck.e2e.providers", JSON.stringify([{
      id: "refresh-models-provider", name: "模型刷新服务", baseUrl: "https://models.example.test/v1",
      protocol: "openai", enabled: true, isCurrent: true, defaultModel: "old-model",
      models: [{ id: "old-model", displayName: "old-model", protocol: "openai", source: "server", capabilities: [] }],
      connectionState: "connected", appliedClients: [],
    }]));
    localStorage.setItem("provider-deck.e2e.secret.refresh-models-provider", "saved-model-key");
  });
  await page.reload({ waitUntil: "domcontentloaded" });
  await openProviders(page, testInfo);
  await page.getByRole("button", { name: "获取模型" }).click();
  await expect(page.getByRole("status")).toContainText("已更新 1 个模型");
  await expect(page.getByText(/1 个模型 · 检测于/)).toBeVisible();
});

test("第三方服务可以执行最小真实对话自测", async ({ page }, testInfo) => {
  await page.evaluate(() => {
    localStorage.setItem("provider-deck.e2e.providers", JSON.stringify([{
      id: "conversation-test-provider", name: "真实对话服务", baseUrl: "https://chat.example.test/v1",
      protocol: "openai", enabled: true, isCurrent: true, defaultModel: "test-coder",
      models: [{ id: "test-coder", displayName: "test-coder", protocol: "openai", source: "server", capabilities: [] }],
      connectionState: "connected", appliedClients: [],
    }]));
  });
  await page.reload({ waitUntil: "domcontentloaded" });
  await openProviders(page, testInfo);
  await page.getByRole("button", { name: "服务自测" }).click();
  await expect(page.getByText(/可能产生少量费用/)).toBeVisible();
  await page.getByRole("button", { name: "开始真实对话自测" }).click();
  await expect(page.getByText("全部测试通过")).toBeVisible();
  await expect(page.getByText("OK", { exact: true })).toBeVisible();
});

test("真实对话失败时展示明确检查结果", async ({ page }, testInfo) => {
  await page.evaluate(() => {
    localStorage.setItem("provider-deck.e2e.fail-provider-test", "1");
    localStorage.setItem("provider-deck.e2e.providers", JSON.stringify([{
      id: "failed-test-provider", name: "失败自测服务", baseUrl: "https://failed.example.test/v1",
      protocol: "openai", enabled: true, isCurrent: true, defaultModel: "missing-model",
      models: [], connectionState: "connected", appliedClients: [],
    }]));
  });
  await page.reload({ waitUntil: "domcontentloaded" });
  await openProviders(page, testInfo);
  await page.getByRole("button", { name: "服务自测" }).click();
  await page.getByRole("button", { name: "开始真实对话自测" }).click();
  await expect(page.getByText("部分测试未通过")).toBeVisible();
  await expect(page.getByText(/模型不可用或无访问权限/)).toBeVisible();
});

// 以下两例断言的是“能力未探明时的回退档位”。每个模型的真实推理档位由该模型自己的
// 能力探测决定（在“编辑服务 → 确认模型”里选），全局设置里只剩这个兜底档位，
// 因此控件名是「回退档位」而不是「推理档位」。该档位只由用户显式选择，没有自动推荐。
test("回退档位可以手动切换并在保存后持久化", async ({ page }, testInfo) => {
  await openSettings(page, testInfo);
  await expect(page.getByLabel("手动回退档位")).toHaveValue("high");
  await page.getByLabel("手动回退档位").selectOption("medium");
  await page.getByRole("button", { name: "保存设置" }).click();
  await expect(page.getByRole("status")).toHaveText("设置已保存");
  await expect(page.getByText("当前回退档位：medium")).toBeVisible();

  await page.reload({ waitUntil: "domcontentloaded" });
  if (testInfo.project.name === "narrow-chromium") await page.getByRole("button", { name: "打开导航" }).click();
  await page.getByRole("button", { name: "设置" }).click();
  await expect(page.getByLabel("手动回退档位")).toHaveValue("medium");
});

test("逐模型兜底档位可以添加、持久化并删除", async ({ page }, testInfo) => {
  await openSettings(page, testInfo);
  await expect(page.getByText("尚未设置逐模型兜底，未探明的模型统一按全局回退档写出。")).toBeVisible();

  await page.getByLabel("兜底模型 ID").fill("relay-coder");
  // 下拉里的取值是档位 id，不再是旧的 low/medium/high。
  await page.getByLabel("兜底档位").selectOption("light");
  await page.getByRole("button", { name: "添加", exact: true }).click();
  await expect(page.getByText("relay-coder", { exact: true })).toBeVisible();
  // 添加后输入框清空，避免连续添加时把上一条模型名带进下一条。
  await expect(page.getByLabel("兜底模型 ID")).toHaveValue("");

  await page.getByRole("button", { name: "保存设置" }).click();
  await expect(page.getByRole("status")).toHaveText("设置已保存");

  await page.reload({ waitUntil: "domcontentloaded" });
  if (testInfo.project.name === "narrow-chromium") await page.getByRole("button", { name: "打开导航" }).click();
  await page.getByRole("button", { name: "设置" }).click();
  await expect(page.getByText("relay-coder", { exact: true })).toBeVisible();

  await page.getByRole("button", { name: "删除 relay-coder 的兜底档位" }).click();
  await page.getByRole("button", { name: "保存设置" }).click();
  await expect(page.getByRole("status")).toHaveText("设置已保存");
  await expect(page.getByText("relay-coder", { exact: true })).toHaveCount(0);
});

// 以下三例断言的是「模型名规则 + 自定义档位」这两级兜底。共同前提：两张表初始为空，
// 不加任何配置时行为与旧版本完全一致，所以第一条断言总是那句空态文案。
test("模型名规则可以添加、按顺序展示并删除", async ({ page }, testInfo) => {
  await openSettings(page, testInfo);
  await expect(page.getByText("尚未设置任何规则。不加规则时行为与旧版本完全一致。")).toBeVisible();

  await page.getByLabel("匹配内容").fill("glm-");
  await page.getByLabel("规则档位").selectOption("light");
  await page.getByRole("button", { name: "添加规则" }).click();

  await page.getByLabel("匹配方式").selectOption("contains");
  await page.getByLabel("匹配内容").fill("thinking");
  await page.getByLabel("规则档位").selectOption("deep");
  await page.getByRole("button", { name: "添加规则" }).click();

  // 顺序即优先级，所以列表必须按添加顺序展示，不排序也不去重。
  const rules = page.locator("ol.fallback-list li");
  await expect(rules).toHaveCount(2);
  await expect(rules.nth(0)).toContainText("glm-");
  await expect(rules.nth(1)).toContainText("thinking");

  await page.getByRole("button", { name: "保存设置" }).click();
  await expect(page.getByRole("status")).toHaveText("设置已保存");

  await page.reload({ waitUntil: "domcontentloaded" });
  if (testInfo.project.name === "narrow-chromium") await page.getByRole("button", { name: "打开导航" }).click();
  await page.getByRole("button", { name: "设置" }).click();
  await expect(page.locator("ol.fallback-list li")).toHaveCount(2);

  await page.getByRole("button", { name: "删除规则 glm-" }).click();
  await expect(page.locator("ol.fallback-list li")).toHaveCount(1);
  await expect(page.locator("ol.fallback-list li").nth(0)).toContainText("thinking");
});

test("自定义档位建好后可被规则引用", async ({ page }, testInfo) => {
  await openSettings(page, testInfo);
  await expect(page.getByText("尚未自建档位。兜底规则可以直接引用内置档位。")).toBeVisible();

  await page.getByRole("button", { name: "新建档位" }).click();
  await page.getByLabel("档位名称").fill("超深推理");
  await page.getByLabel("OpenAI 协议参数").fill('{"reasoning":{"effort":"xhigh"}}');
  await page.getByRole("button", { name: "保存档位" }).click();

  // 名字同时出现在两个档位下拉和档位列表里，所以只断言列表那一处。
  await expect(page.getByRole("listitem").getByText("超深推理", { exact: true })).toBeVisible();
  // 建好之后立刻出现在档位下拉的「自定义档位」分组里。
  await page.getByLabel("规则档位").selectOption({ label: "超深推理" });
  await page.getByLabel("匹配内容").fill("glm-");
  await page.getByRole("button", { name: "添加规则" }).click();
  await expect(page.locator("ol.fallback-list li").nth(0)).toContainText("超深推理");
});

test("删除被引用的档位：规则保留下来并标为已删除", async ({ page }, testInfo) => {
  await openSettings(page, testInfo);
  await page.getByRole("button", { name: "新建档位" }).click();
  await page.getByLabel("档位名称").fill("临时档位");
  await page.getByLabel("OpenAI 协议参数").fill('{"reasoning":{"effort":"high"}}');
  await page.getByRole("button", { name: "保存档位" }).click();

  await page.getByLabel("规则档位").selectOption({ label: "临时档位" });
  await page.getByLabel("匹配内容").fill("glm-");
  await page.getByRole("button", { name: "添加规则" }).click();

  await page.getByRole("button", { name: "删除档位 临时档位" }).click();
  await expect(page.getByText("关联的兜底规则将自动降级", { exact: false })).toBeVisible();
  await expect(page.getByText("当前有 1 条规则引用它", { exact: false })).toBeVisible();
  await page.getByRole("button", { name: "删除档位", exact: true }).click();

  // 规则本身不被删除：结算时跳过这一级继续往下找，界面上标出档位已删除。
  await expect(page.locator("ol.fallback-list li")).toHaveCount(1);
  await expect(page.getByText("档位已删除", { exact: false })).toBeVisible();
});

async function openClients(page: import("@playwright/test").Page, testInfo: import("@playwright/test").TestInfo) {
  await page.evaluate(() => localStorage.setItem("provider-deck.e2e.providers", JSON.stringify([{
    id: "clients-test-provider",
    name: "客户端测试服务",
    baseUrl: "https://clients.example.test/v1",
    protocol: "openai",
    enabled: true,
    isCurrent: true,
    defaultModel: "test-coder",
    models: [],
    connectionState: "connected",
    appliedClients: [],
  }])));
  await page.reload({ waitUntil: "domcontentloaded" });
  if (testInfo.project.name === "narrow-chromium") await page.getByRole("button", { name: "打开导航" }).click();
  await page.getByRole("button", { name: "客户端" }).click();
}

// 以下三例覆盖桌面客户端与 Codex 环境变量鉴权的界面表现。共同前提：浏览器模式下
// clientCatalog 的前两条（codex-cli / claude-code）被标为已安装并带 launchTarget。
test("桌面客户端不提供写入配置的勾选框", async ({ page }, testInfo) => {
  await openClients(page, testInfo);
  // 自动配置的客户端有勾选框。
  await expect(page.getByLabel("选择 OpenAI Codex CLI")).toBeVisible();
  // 桌面端没有：勾了也只会喂给「预览配置」，而它们没有可写的配置文件。
  await expect(page.getByLabel("选择 Claude Desktop")).toHaveCount(0);
  await expect(page.getByLabel("选择 ChatGPT Desktop")).toHaveCount(0);
});

test("桌面客户端展示不修改登录态的引导文案", async ({ page }, testInfo) => {
  await openClients(page, testInfo);
  const guidance = page.getByText("本程序不修改客户端登录态，请在客户端内手动配置 API 地址与密钥", { exact: false });
  await expect(guidance.first()).toBeVisible();
  // 两款桌面端各一条。
  await expect(guidance).toHaveCount(2);
});

test("Codex 卡片说明环境变量免明文配置及其代价", async ({ page }, testInfo) => {
  await openClients(page, testInfo);
  await expect(page.getByText("当前采用环境变量免明文配置，密钥仅在本工具拉起进程时临时注入，独立终端手动执行会提示环境变量缺失。")).toBeVisible();
});

test("配置预览标注环境变量鉴权模式", async ({ page }, testInfo) => {
  await openClients(page, testInfo);
  await page.getByLabel("选择 OpenAI Codex CLI").check();
  await page.getByRole("button", { name: "预览配置" }).click();
  await expect(page.getByText("环境变量鉴权模式，无明文密钥写入本地文件", { exact: false })).toBeVisible();
});

test("修改超时不会重置已保存的推理设置", async ({ page }, testInfo) => {
  await openSettings(page, testInfo);
  await page.getByLabel("手动回退档位").selectOption("low");
  await page.getByRole("button", { name: "保存设置" }).click();
  await expect(page.getByRole("status")).toHaveText("设置已保存");

  await page.getByLabel("请求超时（秒）").fill("45");
  await page.getByRole("button", { name: "保存设置" }).click();
  await expect(page.getByRole("status")).toHaveText("设置已保存");
  await expect(page.getByLabel("手动回退档位")).toHaveValue("low");
  await expect(page.getByText("当前回退档位：low")).toBeVisible();
});

test("设置保存失败时显示明确反馈", async ({ page }, testInfo) => {
  await page.evaluate(() => localStorage.setItem("provider-deck.e2e.fail-settings-save", "1"));
  await openSettings(page, testInfo);
  await page.getByRole("button", { name: "保存设置" }).click();
  await expect(page.getByText("保存失败：测试后端模拟设置保存失败", { exact: true })).toBeVisible();
});

test("窄屏导航和表单不溢出", async ({ page }, testInfo) => {
  test.skip(testInfo.project.name !== "narrow-chromium");
  const bodyBox = await page.locator("body").boundingBox();
  expect(bodyBox?.width).toBeLessThanOrEqual(720);
  await expect(page.getByRole("heading", { name: "添加第一个 AI 服务" })).toBeVisible();
  const dialog = page.getByRole("dialog");
  const box = await dialog.boundingBox();
  expect(box?.x).toBeGreaterThanOrEqual(0);
  expect((box?.x ?? 0) + (box?.width ?? 0)).toBeLessThanOrEqual(720);
});
