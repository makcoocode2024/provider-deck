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

test("推理档位可以手动切换并在保存后持久化", async ({ page }, testInfo) => {
  await openSettings(page, testInfo);
  await expect(page.getByLabel("手动推理档位")).toHaveValue("high");
  await page.getByLabel("手动推理档位").selectOption("medium");
  await page.getByRole("button", { name: "保存设置" }).click();
  await expect(page.getByRole("status")).toHaveText("设置已保存");
  await expect(page.getByText("当前生效：中度")).toBeVisible();

  await page.reload({ waitUntil: "domcontentloaded" });
  if (testInfo.project.name === "narrow-chromium") await page.getByRole("button", { name: "打开导航" }).click();
  await page.getByRole("button", { name: "设置" }).click();
  await expect(page.getByLabel("手动推理档位")).toHaveValue("medium");
});

test("开启自动推荐会隐藏手动档位并保留已选档位", async ({ page }, testInfo) => {
  await openSettings(page, testInfo);
  await page.getByLabel("手动推理档位").selectOption("low");
  await page.getByRole("switch", { name: "自动推荐推理档位" }).click();
  await expect(page.getByLabel("手动推理档位")).toBeHidden();
  await page.getByRole("button", { name: "保存设置" }).click();
  await expect(page.getByRole("status")).toHaveText("设置已保存");

  await page.getByRole("switch", { name: "自动推荐推理档位" }).click();
  await expect(page.getByLabel("手动推理档位")).toHaveValue("low");
});

test("修改超时不会重置已保存的推理设置", async ({ page }, testInfo) => {
  await openSettings(page, testInfo);
  await page.getByLabel("手动推理档位").selectOption("low");
  await page.getByRole("button", { name: "保存设置" }).click();
  await expect(page.getByRole("status")).toHaveText("设置已保存");

  await page.getByLabel("请求超时（秒）").fill("45");
  await page.getByRole("button", { name: "保存设置" }).click();
  await expect(page.getByRole("status")).toHaveText("设置已保存");
  await expect(page.getByLabel("手动推理档位")).toHaveValue("low");
  await expect(page.getByText("当前生效：轻度")).toBeVisible();
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
