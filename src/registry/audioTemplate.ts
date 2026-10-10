import type { ParameterDefinition } from "../types";

export const audioCapabilities = {
  textToSpeech: true,
  speechToText: false,
  speechToSpeech: false,
  voiceClone: false,
};

const voices = [
  "alloy",
  "ash",
  "coral",
  "echo",
  "fable",
  "nova",
  "onyx",
  "sage",
  "shimmer",
];

const formats = ["mp3", "wav", "flac", "opus"];

export const audioParameters: ParameterDefinition[] = [
  {
    key: "voice",
    label: "Voice",
    type: "select",
    options: voices.map((voice) => ({ label: voice, value: voice })),
    default: "alloy",
  },
  {
    key: "speed",
    label: "Speed",
    type: "number",
    min: 0.25,
    max: 4,
    step: 0.05,
    default: 1,
  },
  {
    key: "response_format",
    label: "Format",
    type: "select",
    options: formats.map((format) => ({ label: format, value: format })),
    default: "mp3",
  },
];
