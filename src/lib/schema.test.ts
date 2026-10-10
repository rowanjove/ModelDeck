import { describe, expect, it } from "vitest";
import type { ConstraintDefinition, ParameterDefinition } from "../types";
import {
  collectParameters,
  constraintMessages,
  initialParameterValues,
  isParameterVisible,
  issuesFor,
  parseExtra,
  supportsAudioGenerate,
  supportsAudioTask,
  supportsImageGenerate,
  supportsTextGenerate,
  supportsVideoGenerate,
  withDefaults,
} from "./schema";

const parameters: ParameterDefinition[] = [
  {
    key: "temperature",
    label: "Temperature",
    type: "range",
    min: 0,
    max: 2,
    default: 0.7,
  },
  { key: "seed", label: "Seed", type: "number" },
  { key: "stop", label: "Stop", type: "text" },
  { key: "stream_hint", label: "Hint", type: "boolean", default: true },
];

describe("image providers", () => {
  it("runs OpenAI and ComfyUI, not Ollama", () => {
    expect(supportsImageGenerate("openai")).toBe(true);
    expect(supportsImageGenerate("openai-compatible")).toBe(true);
    expect(supportsImageGenerate("comfyui")).toBe(true);
    expect(supportsImageGenerate("replicate")).toBe(true);
    expect(supportsImageGenerate("fal")).toBe(true);
    expect(supportsImageGenerate("ollama")).toBe(false);
    expect(supportsImageGenerate("generic-http")).toBe(false);
    expect(supportsImageGenerate("anthropic")).toBe(false);
  });
});

describe("video providers", () => {
  it("runs ComfyUI and Generic HTTP", () => {
    expect(supportsVideoGenerate("comfyui")).toBe(true);
    expect(supportsVideoGenerate("generic-http")).toBe(true);
    expect(supportsVideoGenerate("replicate")).toBe(true);
    expect(supportsVideoGenerate("fal")).toBe(true);
    expect(supportsVideoGenerate("openai")).toBe(false);
    expect(supportsVideoGenerate("ollama")).toBe(false);
  });
});

describe("audio providers", () => {
  it("runs OpenAI and Generic HTTP", () => {
    expect(supportsAudioGenerate("openai")).toBe(true);
    expect(supportsAudioGenerate("openai-compatible")).toBe(true);
    expect(supportsAudioGenerate("generic-http")).toBe(true);
    expect(supportsAudioGenerate("replicate")).toBe(true);
    expect(supportsAudioGenerate("fal")).toBe(true);
    expect(supportsAudioGenerate("comfyui")).toBe(false);
    expect(supportsAudioGenerate("ollama")).toBe(false);
    expect(supportsAudioTask("openai", "stt")).toBe(true);
    expect(supportsAudioTask("openai", "sts")).toBe(false);
    expect(supportsAudioTask("openai", "clone")).toBe(false);
    expect(supportsAudioTask("generic-http", "clone")).toBe(true);
    expect(supportsTextGenerate("anthropic")).toBe(true);
    expect(supportsTextGenerate("gemini")).toBe(true);
    expect(supportsTextGenerate("comfyui")).toBe(false);
  });
});

describe("parameter values", () => {
  it("fills range and boolean defaults and omits empty text", () => {
    const initial = initialParameterValues(parameters);
    expect(initial.temperature).toBe(0.7);
    expect(initial.stream_hint).toBe(true);
    expect(initial.seed).toBeUndefined();

    const collected = collectParameters(parameters, { ...initial, stop: "  " });
    expect(collected.temperature).toBe(0.7);
    expect(collected.stream_hint).toBe(true);
    expect(collected.stop).toBeUndefined();
  });

  it("reports mutually exclusive fields after defaults", () => {
    const values = withDefaults(parameters, { seed: 1, stop: "END" });
    const messages = constraintMessages(
      [{ type: "mutuallyExclusive", fields: ["seed", "stop"] }],
      values,
      parameters,
    );
    expect(messages[0]).toContain("不能同时设置");
  });

  it("handles conditional visibility and skips required checks when hidden", () => {
    const testParams: ParameterDefinition[] = [
      {
        key: "mode",
        label: "Mode",
        type: "select",
        options: [
          { label: "Standard", value: "std" },
          { label: "Advanced", value: "adv" },
        ],
        default: "std",
      },
      {
        key: "guidance",
        label: "Guidance Scale",
        type: "number",
        required: true,
      },
    ];

    const constraints: ConstraintDefinition[] = [
      {
        type: "conditionalVisibility",
        field: "guidance",
        dependsOn: "mode",
        equals: "adv",
      },
    ];

    // In std mode: guidance is hidden and required check is skipped
    const stdValues = { mode: "std" };
    expect(isParameterVisible("guidance", constraints, stdValues)).toBe(false);
    const stdIssues = issuesFor(testParams, constraints, stdValues);
    expect(stdIssues.length).toBe(0);

    // In adv mode: guidance is visible and required
    const advValues = { mode: "adv" };
    expect(isParameterVisible("guidance", constraints, advValues)).toBe(true);
    const advIssues = issuesFor(testParams, constraints, advValues);
    expect(advIssues.some((msg) => msg.includes("必填项"))).toBe(true);
  });

  it("enforces requires constraint when trigger parameter is present", () => {
    const testParams: ParameterDefinition[] = [
      { key: "enable_upscale", label: "放大", type: "boolean" },
      { key: "upscale_factor", label: "放大倍数", type: "number" },
    ];

    const constraints: ConstraintDefinition[] = [
      {
        type: "requires",
        field: "enable_upscale",
        requiredFields: ["upscale_factor"],
      },
    ];

    // Trigger is false/absent: no issues
    expect(
      constraintMessages(constraints, { enable_upscale: false }, testParams)
        .length,
    ).toBe(0);

    // Trigger is true, requiredField absent: error
    const msgs = constraintMessages(
      constraints,
      { enable_upscale: true },
      testParams,
    );
    expect(msgs.length).toBe(1);
    expect(msgs[0]).toContain("设置了【放大】时，必须同时提供【放大倍数】");

    // Both present: no errors
    const okMsgs = constraintMessages(
      constraints,
      { enable_upscale: true, upscale_factor: 2 },
      testParams,
    );
    expect(okMsgs.length).toBe(0);
  });

  it("parses extra json objects only", () => {
    expect(parseExtra("")).toEqual({ ok: true, value: null });
    expect(parseExtra('{"seed":1}').ok).toBe(true);
    expect(parseExtra("[1]").ok).toBe(false);
    expect(parseExtra("{").ok).toBe(false);
  });
});
