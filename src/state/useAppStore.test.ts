import { describe, expect, it } from "vitest";
import type { ProviderDraft } from "../domain/types";
import { normalizeProviderDraft } from "./useAppStore";

const draft = (protocolHint: ProviderDraft["protocolHint"]): ProviderDraft => ({
  name: "测试服务",
  baseUrl: "https://api.example.test",
  apiKey: "test-key",
  protocolHint,
  timeoutSeconds: 10,
});

describe("normalizeProviderDraft", () => {
  it("将表单的空协议值转换为未指定协议", () => {
    expect(normalizeProviderDraft(draft("" as ProviderDraft["protocolHint"])).protocolHint).toBeUndefined();
  });

  it("保留明确的协议值并返回新对象", () => {
    const original = draft("anthropic");
    const normalized = normalizeProviderDraft(original);
    expect(normalized).toEqual(original);
    expect(normalized).not.toBe(original);
  });
});
