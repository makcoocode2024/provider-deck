// @vitest-environment jsdom
import { beforeEach, describe, expect, it } from "vitest";
import { defaultSettings } from "../domain/types";
import { backend } from "./backend";

const settingsKey = "provider-deck.e2e.settings";

describe("浏览器测试后端的设置读写", () => {
  beforeEach(() => localStorage.clear());

  it("读取缺少推理字段的旧数据时补齐默认值", async () => {
    localStorage.setItem(settingsKey, JSON.stringify({
      timeoutSeconds: 20,
      proxyUrl: "",
      allowSelfSignedCertificates: false,
      generateOnly: true,
      clearClipboardSeconds: 30,
      locale: "zh-CN",
    }));

    const settings = await backend.getSettings();

    expect(settings.timeoutSeconds).toBe(20);
    expect(settings.autoReasoningMode).toBe(defaultSettings.autoReasoningMode);
    expect(settings.manualReasoningLevel).toBe(defaultSettings.manualReasoningLevel);
    expect(settings.effectiveReasoningLevel).toBe(defaultSettings.effectiveReasoningLevel);
  });

  it("手动模式下把生效档位结算为所选档位并原样返回", async () => {
    const saved = await backend.saveSettings({
      ...defaultSettings,
      timeoutSeconds: 45,
      autoReasoningMode: false,
      manualReasoningLevel: "medium",
    });

    expect(saved.timeoutSeconds).toBe(45);
    expect(saved.manualReasoningLevel).toBe("medium");
    expect(saved.effectiveReasoningLevel).toBe("medium");
    expect(saved.reasoningMatchMessage).toBeUndefined();
    expect((await backend.getSettings()).manualReasoningLevel).toBe("medium");
  });

  it("自动模式下保留手动档位并说明测试后端不做真实推荐", async () => {
    const saved = await backend.saveSettings({ ...defaultSettings, autoReasoningMode: true, manualReasoningLevel: "low" });

    expect(saved.autoReasoningMode).toBe(true);
    expect(saved.manualReasoningLevel).toBe("low");
    expect(saved.reasoningMatchMessage).toContain("浏览器测试模式");
  });
});
