import type { ParameterDefinition } from "../types";

export const imageCapabilities = {
  generate: true,
  imageToImage: false,
  edit: false,
  mask: false,
  referenceImages: false,
};

export const imageParameters: ParameterDefinition[] = [
  {
    key: "width",
    label: "Width",
    type: "number",
    min: 64,
    max: 2048,
    step: 64,
    default: 1024,
  },
  {
    key: "height",
    label: "Height",
    type: "number",
    min: 64,
    max: 2048,
    step: 64,
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
  {
    key: "steps",
    label: "Steps",
    type: "number",
    min: 1,
    max: 150,
    step: 1,
    default: 20,
    advanced: true,
  },
  {
    key: "cfg_scale",
    label: "CFG Scale",
    type: "number",
    min: 0,
    max: 30,
    step: 0.5,
    default: 7,
    advanced: true,
  },
];
