import type { ParameterDefinition } from "../types";

export const textCapabilities = {
  streaming: true,
  vision: false,
  tools: false,
  reasoning: false,
};

export const textParameters: ParameterDefinition[] = [
  {
    key: "temperature",
    label: "Temperature",
    type: "range",
    min: 0,
    max: 2,
    step: 0.1,
    default: 0.7,
  },
  {
    key: "top_p",
    label: "Top P",
    type: "range",
    min: 0,
    max: 1,
    step: 0.05,
    default: 1,
  },
  {
    key: "max_tokens",
    label: "Max Tokens",
    type: "number",
    min: 1,
    max: 32768,
    step: 1,
    default: 1024,
  },
  {
    key: "seed",
    label: "Seed",
    type: "number",
    min: 0,
    step: 1,
    advanced: true,
  },
  { key: "stop", label: "Stop", type: "text", advanced: true },
  {
    key: "frequency_penalty",
    label: "Frequency Penalty",
    type: "range",
    min: -2,
    max: 2,
    step: 0.1,
    default: 0,
    advanced: true,
  },
  {
    key: "presence_penalty",
    label: "Presence Penalty",
    type: "range",
    min: -2,
    max: 2,
    step: 0.1,
    default: 0,
    advanced: true,
  },
  {
    key: "reasoning_effort",
    label: "Reasoning Effort",
    type: "select",
    options: [
      { label: "Low", value: "low" },
      { label: "Medium", value: "medium" },
      { label: "High", value: "high" },
    ],
    advanced: true,
  },
  {
    key: "response_format",
    label: "Response Format",
    type: "select",
    options: [
      { label: "Text", value: "text" },
      { label: "JSON", value: "json_object" },
    ],
    advanced: true,
  },
];
