export interface SuggestedBinding {
  key: string;
  node: string;
  input: string;
}

export function suggestWorkflowBindings(
  workflow: Record<string, unknown>,
  modality: string,
): SuggestedBinding[] {
  const used = new Set<string>();
  const suggestions: SuggestedBinding[] = [];
  const textHits: Array<{ node: string; input: string }> = [];

  function add(key: string, node: string, input: string) {
    if (used.has(key)) {
      return;
    }
    used.add(key);
    suggestions.push({ key, node, input });
  }

  for (const [nodeId, node] of Object.entries(workflow)) {
    if (!node || typeof node !== "object" || Array.isArray(node)) {
      continue;
    }
    const record = node as { class_type?: unknown; inputs?: unknown };
    const classType =
      typeof record.class_type === "string" ? record.class_type : "";
    const inputs = record.inputs;
    if (!inputs || typeof inputs !== "object" || Array.isArray(inputs)) {
      continue;
    }
    const textNode =
      classType.toLowerCase().includes("clip") ||
      classType.toLowerCase().includes("text");
    for (const [inputName, value] of Object.entries(inputs)) {
      if (Array.isArray(value)) {
        continue;
      }
      if (inputName === "seed" || inputName === "noise_seed") {
        add("seed", nodeId, inputName);
      }
      if (inputName === "width") {
        add("width", nodeId, inputName);
      }
      if (inputName === "height") {
        add("height", nodeId, inputName);
      }
      if (inputName === "frames") {
        add("frames", nodeId, inputName);
      }
      if (inputName === "image" && typeof value === "string") {
        if (modality === "image") {
          add("sourceImage", nodeId, inputName);
        }
        if (modality === "video") {
          add("firstFrame", nodeId, inputName);
        }
      }
      if (inputName === "mask" && typeof value === "string") {
        add("mask", nodeId, inputName);
      }
      if (typeof value === "string" && (textNode || inputName === "text")) {
        textHits.push({ node: nodeId, input: inputName });
      }
    }
  }
  const first = textHits[0];
  if (first) {
    add("prompt", first.node, first.input);
  }
  const second = textHits[1];
  if (second && (modality === "image" || modality === "video")) {
    add("negativePrompt", second.node, second.input);
  }
  return suggestions;
}
