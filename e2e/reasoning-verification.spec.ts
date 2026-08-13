import { expect, test } from "@playwright/test";

type Page = import("@playwright/test").Page;
type TestInfo = import("@playwright/test").TestInfo;

test.beforeEach(async ({ page }) => {
  await page.goto("/", { waitUntil: "domcontentloaded" });
  await page.evaluate(() => localStorage.clear());
  await page.reload({ waitUntil: "domcontentloaded" });
});

async function openProviders(page: Page, testInfo: TestInfo) {
  if (testInfo.project.name === "narrow-chromium") {
    await page.getByRole("button", { name: "打开导航" }).click();
  }
  await page.getByRole("button", { name: "Provider" }).click();
}

/** 推理档位区。`getByLabel` 会子串命中内部的「可用推理档位」radiogroup，必须 exact。 */
const picker = (page: Page) => page.getByLabel("推理档位", { exact: true });
const verificationPanel = (page: Page) => page.getByLabel("运行时验证", { exact: true });
/** 顶部徽章：当前档位最新一条的三态文案。历史列表里也有同样的文案，所以必须限定在 header 内。 */
const latestBadge = (page: Page) => page.locator(".reasoning-verification-header .verification-badge");
const latestDetail = (page: Page) => page.locator(".reasoning-verification-detail");
/** header 右侧的能力置信度。页面上「服务端声明」还出现在「服务端声明的全部取值」摘要里。 */
const confidenceBadge = (page: Page) => page.locator(".reasoning-confidence");

const KEY = "test-verify-key";

/**
 * 把编辑向导推进到「确认模型」步。
 *
 * 向导打开时停在 form 步，只有走完一次探测才切到 models 步。先等已保存密钥落到输入框，
 * 否则按钮还是「使用已保存密钥检测」，且 zod 会因 apiKey 为空拦下来。
 */
async function reopenWizardAtModelsStep(page: Page, testInfo: TestInfo) {
  await openProviders(page, testInfo);
  await page.getByRole("button", { name: "编辑" }).click();
  await expect(page.getByLabel("API Key")).toHaveValue(KEY);
  await page.getByRole("button", { name: "开始自动检测" }).click();
  await expect(page.getByRole("heading", { name: "已识别服务" })).toBeVisible();
}

/**
 * 走完整用户流程建出一个已保存的 Provider，并把编辑向导停在推理档位页面。
 *
 * 刻意不用 localStorage 播种：验证需要 stored provider 的 `models[].reasoning`
 * 存在（测试后端要按 tier 从能力表派生 binding），走真实保存路径才能保证这一点。
 * 此时还没有任何验证历史，只有探测到的 `effortEnum` 能力样本。
 */
async function createProviderAndOpenPicker(page: Page, testInfo: TestInfo) {
  await page.getByLabel("服务名称").fill("验证测试服务");
  await page.getByLabel("Base URL").fill("https://verify.example.test/v1");
  await page.getByLabel("API Key").fill(KEY);
  await page.getByRole("button", { name: "开始自动检测" }).click();
  await expect(page.getByRole("heading", { name: "已识别服务" })).toBeVisible();
  await page.getByRole("button", { name: "保存服务" }).click();
  await expect(page.getByRole("heading", { name: "客户端" })).toBeVisible();

  await reopenWizardAtModelsStep(page, testInfo);
}

// 档位文案全部来自测试后端的 effortEnum 样本（backend.ts:38-49）：defaultTier 是 standard，
// 它的 label 是「中度」。组件单测里的「标准推理」是那边自己的 fixture，两套不通用。
const VERIFY_BUTTON = /验证「中度」档位/;

test("已保存服务的推理档位页面存在验证入口", async ({ page }, testInfo) => {
  await createProviderAndOpenPicker(page, testInfo);

  await expect(picker(page)).toBeVisible();
  await expect(verificationPanel(page)).toBeVisible();
  await expect(page.getByText("尚未验证")).toBeVisible();
  await expect(page.getByRole("button", { name: VERIFY_BUTTON })).toBeEnabled();
  await expect(page.getByText(/会向该端点发送一次真实请求，可能产生 API 使用费用/)).toBeVisible();
});

test("点击验证后显示 confirmed，刷新页面后历史仍在", async ({ page }, testInfo) => {
  // 不设 verify-result 开关，测试后端默认返回 confirmed。
  await createProviderAndOpenPicker(page, testInfo);

  await page.getByRole("button", { name: VERIFY_BUTTON }).click();
  await expect(latestBadge(page)).toHaveText("已验证 中度");

  // Confirmed 不得抬高能力置信度，也不得被读成官方支持。
  await expect(confidenceBadge(page)).toHaveText("服务端声明");
  const body = await page.textContent("body");
  expect(body).not.toContain("真实响应证实");
  expect(body).not.toContain("官方支持");

  // 真实刷新：store 清空后从 localStorage 重新 hydrate，走的是持久化路径。
  await page.reload({ waitUntil: "domcontentloaded" });
  await reopenWizardAtModelsStep(page, testInfo);
  await expect(latestBadge(page)).toHaveText("已验证 中度");
  await expect(confidenceBadge(page)).toHaveText("服务端声明");
});

test("rejected 显示未检测到推理产物并持久化，不出现「不支持」", async ({ page }, testInfo) => {
  await page.evaluate(() => localStorage.setItem("provider-deck.e2e.verify-result", "rejected"));
  await createProviderAndOpenPicker(page, testInfo);

  await page.getByRole("button", { name: VERIFY_BUTTON }).click();
  await expect(latestBadge(page)).toHaveText("此 endpoint 下「中度」未检测到推理产物");
  await expect(latestDetail(page)).toContainText("响应中未检测到 openai 协议的推理字段");

  // Rejected ≠ Unsupported。整块验证区不许出现「不支持」，整页不许出现「不支持推理」。
  expect(await verificationPanel(page).textContent()).not.toContain("不支持");
  expect(await page.textContent("body")).not.toContain("不支持推理");
  await expect(confidenceBadge(page)).toHaveText("服务端声明");

  await page.reload({ waitUntil: "domcontentloaded" });
  await reopenWizardAtModelsStep(page, testInfo);
  await expect(latestBadge(page)).toHaveText("此 endpoint 下「中度」未检测到推理产物");
});

test("failed 显示验证失败与错误原文并持久化，不出现「不支持」", async ({ page }, testInfo) => {
  await page.evaluate(() => localStorage.setItem("provider-deck.e2e.verify-result", "failed"));
  await createProviderAndOpenPicker(page, testInfo);

  await page.getByRole("button", { name: VERIFY_BUTTON }).click();
  await expect(latestBadge(page)).toHaveText("验证失败");
  await expect(latestDetail(page)).toContainText("API 错误 429：测试后端模拟请求失败");

  // Failed 同样不是能力结论。
  expect(await verificationPanel(page).textContent()).not.toContain("不支持");
  expect(await page.textContent("body")).not.toContain("不支持推理");
  await expect(confidenceBadge(page)).toHaveText("服务端声明");

  await page.reload({ waitUntil: "domcontentloaded" });
  await reopenWizardAtModelsStep(page, testInfo);
  await expect(latestBadge(page)).toHaveText("验证失败");
  await expect(latestDetail(page)).toContainText("API 错误 429：测试后端模拟请求失败");
});

test("三态连续验证会累积历史，且始终不改变能力置信度", async ({ page }, testInfo) => {
  await createProviderAndOpenPicker(page, testInfo);

  const setResult = (value: string) => page.evaluate(
    (forced) => localStorage.setItem("provider-deck.e2e.verify-result", forced),
    value,
  );

  await setResult("confirmed");
  await page.getByRole("button", { name: VERIFY_BUTTON }).click();
  await expect(latestBadge(page)).toHaveText("已验证 中度");
  await expect(confidenceBadge(page)).toHaveText("服务端声明");

  await setResult("rejected");
  await page.getByRole("button", { name: VERIFY_BUTTON }).click();
  await expect(latestBadge(page)).toHaveText("此 endpoint 下「中度」未检测到推理产物");
  await expect(confidenceBadge(page)).toHaveText("服务端声明");

  await setResult("failed");
  await page.getByRole("button", { name: VERIFY_BUTTON }).click();
  await expect(latestBadge(page)).toHaveText("验证失败");
  await expect(confidenceBadge(page)).toHaveText("服务端声明");

  // 三条都留痕：Rejected / Failed 不是异常，抹掉它们等于让用户反复点同一个按钮。
  const history = page.locator(".reasoning-verification-history");
  await expect(history.locator("summary")).toHaveText("验证历史（3 条）");
  await history.locator("summary").click();
  await expect(history.locator(".verification-badge")).toHaveCount(3);
  expect(await history.textContent()).not.toContain("不支持");

  // 刷新后三条历史全在，顺序保持追加语义（最后一条仍是 failed）。
  await page.reload({ waitUntil: "domcontentloaded" });
  await reopenWizardAtModelsStep(page, testInfo);
  await expect(page.locator(".reasoning-verification-history summary")).toHaveText("验证历史（3 条）");
  await expect(latestBadge(page)).toHaveText("验证失败");
});

// —— 未探明模型的自定义档位入口。
//
// 走 anthropic 协议是因为测试后端只在那条分支返回 `agnes-2.0-lite`（support: unknown），
// 而"未探明"正是新建入口出现的前提条件。

const LITE = "agnes-2.0-lite";
const tierGroups = (page: Page) => page.locator(".reasoning-tier-groups");
/**
 * 档位弹窗。用 class 而不是 `getByLabel`：Radix 会由 `Dialog.Title` 生成
 * `aria-labelledby`，它压过 Content 上的 `aria-label`，所以弹窗的可及名字是
 * 标题文字（新建/编辑档位），按 `aria-label` 找不到。
 */
const tierDialog = (page: Page) => page.locator(".tier-dialog");

/** 建一个 anthropic 服务，并把编辑向导停在 `agnes-2.0-lite` 的档位区。 */
async function createRelayAndSelectLite(page: Page, testInfo: TestInfo) {
  await page.getByLabel("服务名称").fill("未探明档位服务");
  await page.getByLabel("Base URL").fill("https://lite.example.test");
  await page.getByLabel("API Key").fill(KEY);
  await page.getByText("高级选项").click();
  await page.getByLabel("协议提示").selectOption("anthropic");
  await page.getByRole("button", { name: "开始自动检测" }).click();
  await page.getByRole("combobox", { name: "默认模型", exact: true }).selectOption(LITE);
  await page.getByRole("button", { name: "保存服务" }).click();
  await expect(page.getByRole("heading", { name: "客户端" })).toBeVisible();

  await reopenWizardAtModelsStep(page, testInfo);
  await page.getByRole("combobox", { name: "默认模型", exact: true }).selectOption(LITE);
  // 「能力未探明」同时出现在背后的服务卡片上，必须限定在档位区内。
  await expect(picker(page).getByText("能力未探明")).toBeVisible();
}

test("从模型卡片新建档位后，档位区刷新出该档位并成为写入档位", async ({ page }, testInfo) => {
  await createRelayAndSelectLite(page, testInfo);

  // 未探明且无任何匹配档位：此刻只有全局回退档，且有新建入口。
  await expect(tierGroups(page).getByText("全局回退档")).toBeVisible();
  await expect(tierGroups(page).getByText("匹配到的自定义档位")).toHaveCount(0);
  await page.getByRole("button", { name: "新建自定义档位" }).click();

  // 规则预填当前模型名，用户可以改；这里原样用全名保存。
  const dialog = tierDialog(page);
  await expect(dialog.getByLabel("模型名匹配规则")).toHaveValue(LITE);
  await dialog.getByLabel("档位名称").fill("超深思考");
  // anthropic 端点要写得出参数，填的必须是 anthropic 那一栏。
  await dialog.getByLabel("Anthropic 协议参数").fill('{"thinking":{"type":"enabled","budget_tokens":8192}}');
  await dialog.getByRole("button", { name: "保存档位" }).click();

  // 弹窗关闭、档位出现在「匹配到的自定义档位」段、并成为配置写入档位。
  await expect(dialog).toHaveCount(0);
  await expect(tierGroups(page).getByText("匹配到的自定义档位")).toBeVisible();
  await expect(tierGroups(page).getByText("超深思考")).toBeVisible();
  await expect(page.getByText(/配置写入：超深思考/)).toBeVisible();
  await expect(page.getByText(/仅用于写入配置文件，实时请求不发送推理参数/)).toBeVisible();
  // 它是用户设定，不是探测结论：整页不许出现把它说成事实的措辞。
  const body = await page.textContent("body");
  expect(body).not.toContain("已探明档位");
  expect(body).not.toContain("不支持推理");
});

test("新建档位前后能力置信度标签不变", async ({ page }, testInfo) => {
  await createRelayAndSelectLite(page, testInfo);

  // 未探明模型的置信度标签。新建档位是用户设定，不构成任何探测证据。
  await expect(confidenceBadge(page)).toHaveText("未探明");
  await page.getByRole("button", { name: "新建自定义档位" }).click();
  const dialog = tierDialog(page);
  await dialog.getByLabel("档位名称").fill("兜底档位");
  await dialog.getByLabel("Anthropic 协议参数").fill('{"thinking":{"type":"enabled","budget_tokens":4096}}');
  await dialog.getByRole("button", { name: "保存档位" }).click();

  await expect(tierGroups(page).getByText("兜底档位")).toBeVisible();
  await expect(confidenceBadge(page)).toHaveText("未探明");
  await expect(picker(page).getByText("能力未探明")).toBeVisible();

  // 刷新后仍是未探明：档位落了盘，能力结论没被动过。
  await page.reload({ waitUntil: "domcontentloaded" });
  await reopenWizardAtModelsStep(page, testInfo);
  await page.getByRole("combobox", { name: "默认模型", exact: true }).selectOption(LITE);
  await expect(confidenceBadge(page)).toHaveText("未探明");
});

test("新建 Provider 流程不显示验证入口", async ({ page }) => {
  await page.getByLabel("服务名称").fill("新建服务");
  await page.getByLabel("Base URL").fill("https://new.example.test/v1");
  await page.getByLabel("API Key").fill("test-new-key");
  await page.getByRole("button", { name: "开始自动检测" }).click();
  await expect(page.getByRole("heading", { name: "已识别服务" })).toBeVisible();

  // 档位选择器在（能力来自探测结果），但没有 provider id，整个验证区不渲染。
  await expect(picker(page)).toBeVisible();
  await expect(verificationPanel(page)).toHaveCount(0);
  await expect(page.getByRole("dialog").getByRole("button", { name: /验证/ })).toHaveCount(0);
});
