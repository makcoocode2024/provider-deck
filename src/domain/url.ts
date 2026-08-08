export interface NormalizedUrl {
  value: string;
  warning?: string;
}

export function normalizeBaseUrl(input: string): NormalizedUrl {
  const trimmed = input.trim();
  if (!trimmed) throw new Error("请输入 Base URL");

  const withProtocol = /^[a-z][a-z\d+.-]*:\/\//i.test(trimmed)
    ? trimmed
    : `https://${trimmed}`;

  let url: URL;
  try {
    url = new URL(withProtocol);
  } catch {
    throw new Error("Base URL 格式无效");
  }

  if (!['http:', 'https:'].includes(url.protocol)) {
    throw new Error("Base URL 仅支持 HTTP 或 HTTPS");
  }
  if (url.username || url.password) {
    throw new Error("Base URL 不能包含用户名或密码");
  }
  if (url.search || url.hash) {
    throw new Error("Base URL 不能包含查询参数或片段");
  }

  const path = url.pathname.replace(/\/{2,}/g, "/").replace(/\/$/, "");
  url.pathname = path || "/";
  const value = url.toString().replace(/\/$/, "");

  return {
    value,
    warning: url.protocol === "http:" ? "当前地址使用未加密的 HTTP，请确认它是可信的本地或内网服务。" : undefined,
  };
}

export function redactSecret(value: string, secrets: string[] = []): string {
  let output = value;
  for (const secret of secrets.filter((item) => item.length >= 4)) {
    output = output.split(secret).join("[REDACTED]");
  }
  output = output.replace(/(sk-|api[_-]?key[=: ]+|bearer\s+)[A-Za-z0-9._-]{8,}/gi, "$1[REDACTED]");
  return output;
}

export function maskSecret(value: string): string {
  if (!value) return "未保存";
  if (value.length <= 8) return "••••••••";
  return `${value.slice(0, 3)}••••••••${value.slice(-3)}`;
}
