import { describe, expect, it } from "vitest";
import type { ParameterDefinition } from "../../types";
import { getModelVerificationLevel, summarizeModelLimits } from "./ModelsView";

describe("T14: Model capability, verification level and limit extraction", () => {
  it("should classify verification levels accurately aligned with T07", () => {
    // L4 verified real service
    const ollamaL4 = getModelVerificationLevel("ollama", "llama3.1");
    expect(ollamaL4.level).toBe("L4");
    expect(ollamaL4.label).toContain("L4");

    const gptL4 = getModelVerificationLevel("openai", "gpt-4o");
    expect(gptL4.level).toBe("L4");

    // L3 desktop E2E verified
    const comfyL3 = getModelVerificationLevel("comfyui", "flux-dev");
    expect(comfyL3.level).toBe("L3");

    // L2 contract covered
    const compatL2 = getModelVerificationLevel(
      "openaicompatible",
      "custom-llm",
    );
    expect(compatL2.level).toBe("L2");

    // L1 static definition
    const customL1 = getModelVerificationLevel(
      "generic_http",
      "my-private-model",
    );
    expect(customL1.level).toBe("L1");
  });

  it("should summarize video model constraints and limits accurately", () => {
    const videoParams: ParameterDefinition[] = [
      {
        key: "seconds",
        label: "视频时长",
        type: "number",
        max: 10,
      },
      {
        key: "size",
        label: "分辨率",
        type: "select",
        options: [
          { label: "1280x720 (16:9)", value: "1280x720" },
          { label: "720x1280 (9:16)", value: "720x1280" },
        ],
      },
    ];

    const limits = summarizeModelLimits(
      videoParams,
      { firstFrame: true, firstLastFrame: true },
      "video",
    );

    expect(limits.some((l) => l.includes("首尾帧"))).toBe(true);
    expect(limits.some((l) => l.includes("最长时长: 10秒"))).toBe(true);
    expect(limits.some((l) => l.includes("1280x720"))).toBe(true);
  });

  it("should summarize image inpainting and reference constraints", () => {
    const imageParams: ParameterDefinition[] = [
      {
        key: "steps",
        label: "采样步数",
        type: "range",
        max: 50,
      },
      {
        key: "width",
        label: "宽度",
        type: "range",
        min: 512,
        max: 2048,
      },
    ];

    const limits = summarizeModelLimits(
      imageParams,
      { mask: true, referenceImages: true },
      "image",
    );

    expect(limits.some((l) => l.includes("蒙版重绘"))).toBe(true);
    expect(limits.some((l) => l.includes("多参考图"))).toBe(true);
    expect(limits.some((l) => l.includes("步数上限: 50"))).toBe(true);
  });
});
