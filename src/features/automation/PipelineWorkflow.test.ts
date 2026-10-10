import { describe, expect, it } from "vitest";
import type {
  PipelineDefinition,
  PipelineExecutionInput,
  PipelineStepResult,
} from "../../types";

function validatePipelineClient(def: PipelineDefinition): {
  valid: boolean;
  error?: string;
} {
  if (!def.name || !def.name.trim()) {
    return { valid: false, error: "工作流名称不能为空" };
  }
  if (!def.steps || def.steps.length === 0) {
    return { valid: false, error: "工作流至少需要包含一个步骤" };
  }
  const seenIds = new Set<string>();
  for (const step of def.steps) {
    const id = step.id?.trim();
    if (!id) return { valid: false, error: "步骤未指定 ID" };
    if (seenIds.has(id)) return { valid: false, error: `步骤 ID 重复: ${id}` };
    seenIds.add(id);
  }
  return { valid: true };
}

function substituteClientVars(
  template: string,
  vars: Record<string, string>,
): string {
  let res = template;
  for (const [k, v] of Object.entries(vars)) {
    res = res.split(`{{${k}}}`).join(v);
  }
  return res;
}

describe("Pipeline Workflow logic and typing", () => {
  it("validates pipeline definitions correctly", () => {
    expect(
      validatePipelineClient({
        id: "1",
        name: "   ",
        description: "",
        steps: [{ type: "input", id: "s1", label: "input" }],
        createdAt: "",
        updatedAt: "",
      }).valid,
    ).toBe(false);

    expect(
      validatePipelineClient({
        id: "2",
        name: "Valid Pipeline",
        description: "",
        steps: [],
        createdAt: "",
        updatedAt: "",
      }).valid,
    ).toBe(false);

    expect(
      validatePipelineClient({
        id: "3",
        name: "Valid Pipeline",
        description: "",
        steps: [
          { type: "input", id: "same_id", label: "input" },
          {
            type: "transform",
            id: "same_id",
            label: "trans",
            kind: "template",
            rule: "",
          },
        ],
        createdAt: "",
        updatedAt: "",
      }).valid,
    ).toBe(false);

    expect(
      validatePipelineClient({
        id: "4",
        name: "Valid Pipeline",
        description: "",
        steps: [
          { type: "input", id: "s1", label: "input", dataType: "text" },
          {
            type: "model",
            id: "s2",
            label: "model",
            modelDefinitionId: "m1",
            promptTemplate: "{{prev}}",
            expectedInputKind: "image",
            mediaInputVariable: "{{s1.media}}",
          },
        ],
        createdAt: "",
        updatedAt: "",
      }).valid,
    ).toBe(true);
  });

  it("substitutes workflow variables including media references", () => {
    const vars: Record<string, string> = {
      input: "Cyberpunk concept",
      prev: "A glowing futuristic tower",
      "step_keyframe.media": "C:/app_data/assets/tower.png",
    };

    const promptTpl = "Cinematic scene: {{prev}}, inspired by {{input}}";
    expect(substituteClientVars(promptTpl, vars)).toBe(
      "Cinematic scene: A glowing futuristic tower, inspired by Cyberpunk concept",
    );

    const mediaRefTpl = "{{step_keyframe.media}}";
    expect(substituteClientVars(mediaRefTpl, vars)).toBe(
      "C:/app_data/assets/tower.png",
    );
  });

  it("constructs breakpoint resumption payload properly", () => {
    const cached: PipelineStepResult[] = [
      {
        stepId: "step_input",
        stepLabel: "输入",
        stepType: "input",
        outputKind: "text",
        textOutput: "Story script",
        mediaPaths: [],
        latencyMs: 12,
        status: "completed",
      },
      {
        stepId: "step_keyframe",
        stepLabel: "概念关键帧",
        stepType: "model",
        outputKind: "image",
        textOutput: null,
        mediaPaths: ["C:/assets/frame.png"],
        latencyMs: 2500,
        status: "completed",
      },
    ];

    const input: PipelineExecutionInput = {
      pipelineId: "pipe_123",
      steps: [
        { type: "input", id: "step_input", label: "输入" },
        {
          type: "model",
          id: "step_keyframe",
          label: "概念帧",
          modelDefinitionId: "img_m",
          promptTemplate: "{{input}}",
        },
        {
          type: "model",
          id: "step_video",
          label: "生成视频",
          modelDefinitionId: "vid_m",
          promptTemplate: "{{prev}}",
          mediaInputVariable: "{{step_keyframe.media}}",
        },
      ],
      initialInput: "Story script",
      resumeFromStepIndex: 2,
      cachedStepResults: cached,
    };

    expect(input.resumeFromStepIndex).toBe(2);
    expect(input.cachedStepResults).toHaveLength(2);
    expect(input.cachedStepResults?.[1].mediaPaths[0]).toBe(
      "C:/assets/frame.png",
    );
  });
});
