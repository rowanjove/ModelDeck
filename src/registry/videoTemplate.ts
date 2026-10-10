import type { ParameterDefinition } from "../types";

export const videoCapabilities = {
  textToVideo: true,
  imageToVideo: false,
  firstFrame: true,
  lastFrame: true,
  referenceImages: false,
  referenceVideos: false,
  referenceAudio: false,
};

export const videoParameters: ParameterDefinition[] = [
  {
    key: "duration",
    label: "Duration",
    type: "number",
    min: 1,
    max: 30,
    step: 1,
    default: 5,
  },
  {
    key: "width",
    label: "Width",
    type: "number",
    min: 64,
    max: 2048,
    step: 64,
    default: 1280,
  },
  {
    key: "height",
    label: "Height",
    type: "number",
    min: 64,
    max: 2048,
    step: 64,
    default: 720,
  },
  {
    key: "fps",
    label: "FPS",
    type: "number",
    min: 1,
    max: 60,
    step: 1,
    default: 16,
  },
  {
    key: "seed",
    label: "Seed",
    type: "number",
    min: 0,
    step: 1,
    advanced: true,
  },
];
