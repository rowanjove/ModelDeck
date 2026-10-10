import { describe, expect, it } from "vitest";
import { suggestWorkflowBindings } from "./workflow";

describe("workflow bindings", () => {
  it("suggests the first unbound inputs and skips node links", () => {
    const suggestions = suggestWorkflowBindings(
      {
        "6": {
          class_type: "CLIPTextEncode",
          inputs: { text: "a fox", clip: ["4", 0] },
        },
        "7": {
          class_type: "CLIPTextEncode",
          inputs: { text: "blur" },
        },
        "3": {
          class_type: "KSampler",
          inputs: { seed: 1, noise_seed: 2, steps: ["5", 0] },
        },
        "5": {
          class_type: "EmptyLatentImage",
          inputs: { width: 512, height: 512 },
        },
        "8": {
          class_type: "LoadImage",
          inputs: { image: "photo.png" },
        },
        "9": {
          class_type: "LoadImageMask",
          inputs: { mask: "mask.png" },
        },
      },
      "image",
    );
    expect(suggestions).toEqual([
      { key: "seed", node: "3", input: "seed" },
      { key: "width", node: "5", input: "width" },
      { key: "height", node: "5", input: "height" },
      { key: "sourceImage", node: "8", input: "image" },
      { key: "mask", node: "9", input: "mask" },
      { key: "prompt", node: "6", input: "text" },
      { key: "negativePrompt", node: "7", input: "text" },
    ]);
  });

  it("maps a video image input to the first frame", () => {
    const suggestions = suggestWorkflowBindings(
      {
        "1": {
          class_type: "LoadImage",
          inputs: { image: "start.png", frames: 16 },
        },
      },
      "video",
    );
    expect(suggestions.map((item) => item.key)).toEqual([
      "firstFrame",
      "frames",
    ]);
  });
});
