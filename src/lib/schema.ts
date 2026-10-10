import {
  CAPABILITY_FIELDS,
  type ConstraintDefinition,
  type ParameterDefinition,
} from "../types";

export function capabilityEnabled(
  capabilities: Record<string, unknown> | undefined,
  key: string,
): boolean {
  const value = capabilities?.[key];
  if (typeof value === "boolean") {
    return value;
  }
  if (value && typeof value === "object" && "enabled" in value) {
    return Boolean((value as { enabled?: unknown }).enabled);
  }
  return false;
}

export function emptyCapabilities(
  modality: keyof typeof CAPABILITY_FIELDS,
): Record<string, boolean> {
  const capabilities: Record<string, boolean> = {};
  for (const field of CAPABILITY_FIELDS[modality]) {
    capabilities[field.key] = false;
  }
  if (modality === "text") {
    capabilities.streaming = true;
  }
  if (modality === "image") {
    capabilities.generate = true;
  }
  if (modality === "video") {
    capabilities.textToVideo = true;
  }
  if (modality === "audio") {
    capabilities.textToSpeech = true;
  }
  return capabilities;
}

export function isPresent(value: unknown): boolean {
  if (value === undefined || value === null) {
    return false;
  }
  if (typeof value === "string" && value.trim() === "") {
    return false;
  }
  if (Array.isArray(value) && value.length === 0) {
    return false;
  }
  return true;
}

export function initialParameterValues(
  parameters: ParameterDefinition[],
): Record<string, unknown> {
  const values: Record<string, unknown> = {};
  for (const parameter of parameters) {
    if (parameter.type === "boolean") {
      values[parameter.key] =
        typeof parameter.default === "boolean" ? parameter.default : false;
      continue;
    }
    if (parameter.type === "range") {
      values[parameter.key] =
        typeof parameter.default === "number"
          ? parameter.default
          : (parameter.min ?? 0);
      continue;
    }
    if (isPresent(parameter.default)) {
      values[parameter.key] = parameter.default;
    }
  }
  return values;
}

export function withDefaults(
  parameters: ParameterDefinition[],
  values: Record<string, unknown>,
): Record<string, unknown> {
  const next = { ...values };
  for (const parameter of parameters) {
    if (!isPresent(next[parameter.key]) && isPresent(parameter.default)) {
      next[parameter.key] = parameter.default;
    }
  }
  return next;
}

function valuesEqual(a: unknown, b: unknown): boolean {
  if (a === b) return true;
  if (a === undefined && b === null) return true;
  if (a === null && b === undefined) return true;
  return JSON.stringify(a) === JSON.stringify(b);
}

export function isParameterVisible(
  parameterKey: string,
  constraints: ConstraintDefinition[],
  values: Record<string, unknown>,
): boolean {
  for (const constraint of constraints) {
    if (
      constraint.type === "conditionalVisibility" &&
      constraint.field === parameterKey
    ) {
      const parentVal = values[constraint.dependsOn];
      if (constraint.equals !== undefined) {
        if (!valuesEqual(parentVal, constraint.equals)) {
          return false;
        }
      }
      if (constraint.notEquals !== undefined) {
        if (valuesEqual(parentVal, constraint.notEquals)) {
          return false;
        }
      }
    }
  }
  return true;
}

export function collectParameters(
  parameters: ParameterDefinition[],
  values: Record<string, unknown>,
  constraints?: ConstraintDefinition[],
): Record<string, unknown> {
  const collected: Record<string, unknown> = {};
  for (const parameter of parameters) {
    if (
      constraints &&
      !isParameterVisible(parameter.key, constraints, values)
    ) {
      continue;
    }
    const value = values[parameter.key];
    if (parameter.type === "boolean") {
      collected[parameter.key] = typeof value === "boolean" ? value : false;
      continue;
    }
    if (parameter.type === "range") {
      if (typeof value === "number" && !Number.isNaN(value)) {
        collected[parameter.key] = value;
      }
      continue;
    }
    if (!isPresent(value)) {
      continue;
    }
    collected[parameter.key] = value;
  }
  return collected;
}

function isParamActive(value: unknown): boolean {
  if (value === false) return false;
  return isPresent(value);
}

export function constraintMessages(
  constraints: ConstraintDefinition[],
  values: Record<string, unknown>,
  parameters: ParameterDefinition[],
): string[] {
  const labels = new Map(
    parameters.map((parameter) => [parameter.key, parameter.label]),
  );
  const messages: string[] = [];
  for (const constraint of constraints) {
    if (constraint.type === "mutuallyExclusive") {
      const present = constraint.fields.filter(
        (field) =>
          isParameterVisible(field, constraints, values) &&
          isPresent(values[field]),
      );
      if (present.length > 1) {
        const names = present
          .map((field) => labels.get(field) ?? field)
          .join("、");
        messages.push(`${names} 不能同时设置`);
      }
    } else if (constraint.type === "requires") {
      if (
        isParameterVisible(constraint.field, constraints, values) &&
        isParamActive(values[constraint.field])
      ) {
        for (const req of constraint.requiredFields) {
          if (
            !isParameterVisible(req, constraints, values) ||
            !isParamActive(values[req])
          ) {
            const fieldName = labels.get(constraint.field) ?? constraint.field;
            const reqName = labels.get(req) ?? req;
            messages.push(
              `设置了【${fieldName}】时，必须同时提供【${reqName}】`,
            );
          }
        }
      }
    }
  }
  return messages;
}

export function issuesFor(
  parameters: ParameterDefinition[],
  constraints: ConstraintDefinition[],
  values: Record<string, unknown>,
): string[] {
  const effective = withDefaults(
    parameters,
    collectParameters(parameters, values, constraints),
  );
  const issues: string[] = [];
  for (const parameter of parameters) {
    if (!isParameterVisible(parameter.key, constraints, effective)) {
      continue;
    }
    const value = effective[parameter.key];
    if (!isPresent(value)) {
      if (parameter.required) {
        issues.push(`${parameter.label}是必填项`);
      }
      continue;
    }
    if (
      (parameter.type === "number" || parameter.type === "range") &&
      typeof value === "number"
    ) {
      if (parameter.min !== undefined && value < parameter.min) {
        issues.push(`${parameter.label}不能小于 ${parameter.min}`);
      }
      if (parameter.max !== undefined && value > parameter.max) {
        issues.push(`${parameter.label}不能大于 ${parameter.max}`);
      }
    }
  }
  issues.push(...constraintMessages(constraints, effective, parameters));
  return issues;
}

export function parseExtra(
  text: string,
):
  | { ok: true; value: Record<string, unknown> | null }
  | { ok: false; message: string } {
  const trimmed = text.trim();
  if (!trimmed) {
    return { ok: true, value: null };
  }
  try {
    const value: unknown = JSON.parse(trimmed);
    if (!value || typeof value !== "object" || Array.isArray(value)) {
      return { ok: false, message: "Extra JSON 必须是对象" };
    }
    return { ok: true, value: value as Record<string, unknown> };
  } catch {
    return { ok: false, message: "Extra JSON 不是合法 JSON" };
  }
}

export function fileName(path: string): string {
  const parts = path.split(/[/\\]/);
  return parts[parts.length - 1] || path;
}

export function supportsTextGenerate(
  providerType: string | undefined,
): boolean {
  return (
    providerType === "openai" ||
    providerType === "openai-compatible" ||
    providerType === "ollama" ||
    providerType === "anthropic" ||
    providerType === "gemini" ||
    providerType === "replicate" ||
    providerType === "fal"
  );
}

export function supportsImageGenerate(
  providerType: string | undefined,
): boolean {
  return (
    providerType === "openai" ||
    providerType === "openai-compatible" ||
    providerType === "comfyui" ||
    providerType === "replicate" ||
    providerType === "fal"
  );
}

export function supportsVideoGenerate(
  providerType: string | undefined,
): boolean {
  return (
    providerType === "comfyui" ||
    providerType === "generic-http" ||
    providerType === "replicate" ||
    providerType === "fal"
  );
}

export function supportsAudioGenerate(
  providerType: string | undefined,
): boolean {
  return supportsAudioTask(providerType, "tts");
}

export function supportsAudioTask(
  providerType: string | undefined,
  task: string,
): boolean {
  if (task === "sts" || task === "clone") {
    return (
      providerType === "generic-http" ||
      providerType === "replicate" ||
      providerType === "fal"
    );
  }
  return (
    providerType === "openai" ||
    providerType === "openai-compatible" ||
    providerType === "generic-http" ||
    providerType === "replicate" ||
    providerType === "fal"
  );
}

export function capabilityLimit(
  capabilities: Record<string, unknown> | undefined,
  key: string,
  fallback: number,
): number {
  const value = capabilities?.[key];
  if (value && typeof value === "object" && "max" in value) {
    const max = Number((value as { max?: unknown }).max);
    if (Number.isFinite(max) && max >= 1) {
      return Math.min(8, Math.floor(max));
    }
  }
  return fallback;
}

const MEDIA_PARAMETER_TYPES = new Set([
  "image",
  "images",
  "video",
  "videos",
  "audio",
  "audios",
]);

export function scalarParameters<T extends { type: string }>(
  parameters: T[],
): T[] {
  return parameters.filter(
    (parameter) => !MEDIA_PARAMETER_TYPES.has(parameter.type),
  );
}
