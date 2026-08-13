// @vitest-environment jsdom
import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type {
  AppSettings,
  ConfigChange,
  ModelInfo,
  Provider,
  ReasoningCapability,
} from "../domain/types";
import { defaultSettings } from "../domain/types";
import { writeTargetSummary } from "../domain/reasoning";
import { useAppStore } from "../state/useAppStore";
import { ConfigPreview } from "./ConfigPreview";
import { ReasoningTierPicker } from "./ReasoningTierPicker";

// 预览只读 store，不调后端。apply 换成 mock 免得点不到也报错。
const mocks = vi.hoisted(() => ({ apply: vi.fn() }));

const capability: ReasoningCapability = {
  key: { baseUrl: "https://api.example.com/v1", modelId: "test-coder" },
  support: "unknown",
  control: { kind: "none" },
  tiers: [],
  constraints: {},
  confidence: "unknown",
  evidence: [],
  discoveredAt: "2026-08-13T10:00:00Z",
  ttlSeconds: 6 * 3600,
};

const model: ModelInfo = {
  id: "test-coder",
  displayName: "test-coder",
  protocol: "openai",
  source: "server",
  capabilities: [],
  reasoning: capability,
};

function provider(overrides: Partial<Provider> = {}): Provider {
  return {
    id: "p1",
    name: "测试服务",
    baseUrl: "https://api.example.com/v1",
    protocol: "openai",
    enabled: true,
    isCurrent: true,
    defaultModel: "test-coder",
    models: [model],
    connectionState: "connected",
    appliedClients: [],
    ...overrides,
  };
}

const change: ConfigChange = {
  clientId: "codex",
  clientName: "Codex CLI",
  targetPath: "C:\\Users\\tester\\.codex\\config.toml",
  support: "verified",
  canWrite: true,
  format: "toml",
  beforePreview: "- model = \"old\"",
  afterPreview: "+ model = \"test-coder\"",
  warnings: [],
};

function seedStore(overrides: Partial<AppSettings> = {}) {
  useAppStore.setState({
    providers: [],
    clients: [],
    backups: [],
    settings: { ...defaultSettings, ...overrides },
    loading: false,
    operation: undefined,
    error: undefined,
    changes: [change],
    applyResults: [],
    reasoningMeta: {},
    detectingReasoning: {},
    apply: mocks.apply,
  });
}

describe("ConfigPreview 的推理档位说明", () => {
  beforeEach(() => {
    mocks.apply.mockReset();
    seedStore();
  });
  afterEach(() => cleanup());

  it("兜底场景带「未探测」标注与配置文件专属说明", () => {
    seedStore({ effectiveReasoningLevel: "high" });
    render(<ConfigPreview open provider={provider()} onOpenChange={vi.fn()} />);

    expect(screen.getByText(/配置写入：high · 全局回退档（未探测，可新建自定义档位适配此模型）/)).toBeInTheDocument();
    expect(screen.getByText(/仅用于写入配置文件，实时请求不发送推理参数。/)).toBeInTheDocument();
  });

  it("omitted 场景不显示兜底提示，改为说明依探测结论省略", () => {
    const unsupported: ReasoningCapability = { ...capability, support: "unsupported" };
    render(
      <ConfigPreview
        open
        provider={provider({ models: [{ ...model, reasoning: unsupported }] })}
        onOpenChange={vi.fn()}
      />,
    );

    expect(screen.getByText("不写入档位（依探测结论省略）")).toBeInTheDocument();
    // 兜底提示整句缺席：探测已排除写档位，再报一个档位名就是错的。
    expect(screen.queryByText(/配置写入/)).not.toBeInTheDocument();
    expect(screen.queryByText(/全局回退档/)).not.toBeInTheDocument();
  });

  it("没有默认模型时整块说明缺席，diff 仍照常渲染", () => {
    render(<ConfigPreview open provider={provider({ defaultModel: undefined })} onOpenChange={vi.fn()} />);

    expect(screen.queryByText(/配置写入/)).not.toBeInTheDocument();
    expect(screen.queryByText(/不写入档位/)).not.toBeInTheDocument();
    expect(screen.getByText("Codex CLI")).toBeInTheDocument();
  });

  // —— 4.4：跨组件一致性。
  //
  // 两个组件在同一时刻、同一模型下必须说同一句话。断言比对的是**渲染出的文本**，
  // 不是各自再调一次 writeTargetSummary —— 后者只能证明函数是纯的，
  // 证明不了两处组件都真的用了它。
  it("与 ReasoningTierPicker 展示同一个场景与同一个档位名", () => {
    const settings: AppSettings = {
      ...defaultSettings,
      customReasoningTiers: [{ id: "tier-x", label: "超深", openaiParams: { reasoning: { effort: "xhigh" } } }],
      reasoningNameRules: [{ id: "r", pattern: "test-", matchType: "prefix", tierId: "tier-x" }],
    };
    useAppStore.setState({ settings });
    const summary = writeTargetSummary(capability, undefined, settings, "test-coder");
    expect(summary?.scene).toBe("matched-custom");

    render(<ConfigPreview open provider={provider()} onOpenChange={vi.fn()} />);
    const previewNote = screen.getByText(/配置写入：超深/).closest("p")?.textContent;
    cleanup();

    render(<ReasoningTierPicker capability={capability} onChange={vi.fn()} writeTarget={summary} />);
    const pickerNote = screen.getByText(/配置写入：超深/).closest("p")?.textContent;

    expect(previewNote).toBe(pickerNote);
    expect(previewNote).toContain("超深");
    expect(previewNote).toContain("未探测");
    // 设定性取值不许套上事实性措辞，两处都不许。
    for (const word of ["支持", "兼容", "已确认", "已验证", "已探明"]) {
      expect(previewNote).not.toContain(word);
    }
  });

  // —— 4.5：脱敏。
  //
  // 预览里出现过 targetPath、diff、错误消息三类外部文本。这条钉的是"渲染层不引入
  // 密钥"：store 里的密钥形状字符串若被顺手带进 DOM，这里必须失败。
  it("预览文案不含任何密钥或鉴权字段片段", () => {
    useAppStore.setState({
      changes: [{
        ...change,
        warnings: ["目标文件已被外部修改，将重新读取后再合并。"],
      }],
      applyResults: [{
        clientId: "codex",
        success: false,
        message: "写入失败：目标文件被占用（已回滚）",
        restartRequired: false,
      }],
      settings: { ...defaultSettings, effectiveReasoningLevel: "high" },
    });
    render(<ConfigPreview open provider={provider()} onOpenChange={vi.fn()} />);

    const text = document.body.textContent ?? "";
    for (const forbidden of ["sk-", "AIza", "apiKey", "api_key", "Authorization", "Bearer"]) {
      expect(text).not.toContain(forbidden);
    }
    // 兜底说明确实渲染了 —— 否则上面的否定断言会因为"什么都没渲染"而空转通过。
    expect(text).toContain("配置写入：high");
  });
});
