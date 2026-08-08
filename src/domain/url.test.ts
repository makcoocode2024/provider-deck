import { describe, expect, it } from "vitest";
import { maskSecret, normalizeBaseUrl, redactSecret } from "./url";

describe("normalizeBaseUrl", () => {
  it("补全协议并清理末尾斜杠", () => {
    expect(normalizeBaseUrl(" api.example.com/v1/ ").value).toBe("https://api.example.com/v1");
  });

  it("保留显式 HTTP 并给出警告", () => {
    const result = normalizeBaseUrl("http://127.0.0.1:11434/v1/");
    expect(result.value).toBe("http://127.0.0.1:11434/v1");
    expect(result.warning).toContain("HTTP");
  });

  it("拒绝 URL 中的凭据和查询参数", () => {
    expect(() => normalizeBaseUrl("https://user:pass@example.com")).toThrow();
    expect(() => normalizeBaseUrl("https://example.com?key=secret")).toThrow();
  });
});

describe("secret helpers", () => {
  it("脱敏显式密钥", () => {
    expect(redactSecret("token=very-secret-value", ["very-secret-value"])).toBe("token=[REDACTED]");
  });

  it("掩码不暴露完整值", () => {
    expect(maskSecret("sk-1234567890abcdef")).toBe("sk-••••••••def");
  });
});
