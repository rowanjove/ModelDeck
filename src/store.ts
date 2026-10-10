import { create } from "zustand";
import type { Modality } from "./types";

export interface ImageReferenceDraft {
  path: string;
  role: string;
  weight: string;
  description: string;
  enabled?: boolean;
}

interface TextDraft {
  modelDefinitionId: string | null;
  boundModelId: string | null;
  systemPrompt: string;
  prompt: string;
  values: Record<string, unknown>;
  extraText: string;
  images: string[];
  reset: () => void;
  patch: (partial: Partial<Omit<TextDraft, "reset" | "patch">>) => void;
}

const empty = {
  modelDefinitionId: null,
  boundModelId: null,
  systemPrompt: "",
  prompt: "",
  values: {},
  extraText: "",
  images: [],
};

export const useTextDraft = create<TextDraft>((set) => ({
  ...empty,
  reset: () => set(empty),
  patch: (partial) => set(partial),
}));

interface ImageDraft {
  modelDefinitionId: string | null;
  boundModelId: string | null;
  prompt: string;
  negativePrompt: string;
  values: Record<string, unknown>;
  extraText: string;
  sourcePath: string | null;
  references: ImageReferenceDraft[];
  maskPath: string | null;
  activeTaskId: string | null;
  reset: () => void;
  patch: (partial: Partial<Omit<ImageDraft, "reset" | "patch">>) => void;
}

const emptyImage = {
  modelDefinitionId: null,
  boundModelId: null,
  prompt: "",
  negativePrompt: "",
  values: {},
  extraText: "",
  sourcePath: null,
  references: [] as ImageReferenceDraft[],
  maskPath: null,
  activeTaskId: null as string | null,
};

export const useImageDraft = create<ImageDraft>((set) => ({
  ...emptyImage,
  reset: () => set(emptyImage),
  patch: (partial) => set(partial),
}));

interface VideoDraft {
  modelDefinitionId: string | null;
  boundModelId: string | null;
  prompt: string;
  negativePrompt: string;
  values: Record<string, unknown>;
  extraText: string;
  firstPath: string | null;
  lastPath: string | null;
  references: ImageReferenceDraft[];
  videoPaths: string[];
  audioPaths: string[];
  activeTaskId: string | null;
  reset: () => void;
  patch: (partial: Partial<Omit<VideoDraft, "reset" | "patch">>) => void;
}

const emptyVideo = {
  modelDefinitionId: null,
  boundModelId: null,
  prompt: "",
  negativePrompt: "",
  values: {},
  extraText: "",
  firstPath: null,
  lastPath: null,
  references: [] as ImageReferenceDraft[],
  videoPaths: [] as string[],
  audioPaths: [] as string[],
  activeTaskId: null as string | null,
};

export const useVideoDraft = create<VideoDraft>((set) => ({
  ...emptyVideo,
  reset: () => set(emptyVideo),
  patch: (partial) => set(partial),
}));

interface AudioDraft {
  modelDefinitionId: string | null;
  boundModelId: string | null;
  prompt: string;
  task: "tts" | "stt" | "sts" | "clone";
  voiceId: string | null;
  voiceName: string;
  values: Record<string, unknown>;
  extraText: string;
  sourcePath: string | null;
  referencePath: string | null;
  transcript: string;
  activeTaskId: string | null;
  reset: () => void;
  patch: (partial: Partial<Omit<AudioDraft, "reset" | "patch">>) => void;
}

const emptyAudio: Omit<AudioDraft, "reset" | "patch"> = {
  modelDefinitionId: null,
  boundModelId: null,
  prompt: "",
  task: "tts",
  voiceId: null,
  voiceName: "",
  values: {},
  extraText: "",
  sourcePath: null,
  referencePath: null,
  transcript: "",
  activeTaskId: null,
};

export const useAudioDraft = create<AudioDraft>((set) => ({
  ...emptyAudio,
  reset: () => set(emptyAudio),
  patch: (partial) => set(partial),
}));

export interface RestorePayload {
  nonce: number;
  kind: "generate" | "chat" | "image" | "video" | "audio";
  modelDefinitionId: string;
  systemPrompt: string;
  prompt: string;
  parameters: Record<string, unknown>;
  extraText: string;
  images: string[];
  conversationId: string | null;
  negativePrompt?: string;
  sourcePath?: string | null;
  references?: ImageReferenceDraft[];
  maskPath?: string | null;
  firstPath?: string | null;
  lastPath?: string | null;
  videoPaths?: string[];
  audioPaths?: string[];
  voiceName?: string;
  referencePath?: string | null;
  transcript?: string;
  task?: "tts" | "stt" | "sts" | "clone";
}

interface RestoreStore {
  request: RestorePayload | null;
  publish: (payload: Omit<RestorePayload, "nonce">) => void;
  clear: () => void;
}

interface ModelPick {
  nonce: number;
  modelId: string;
  modality: Modality;
}

interface ModelPickStore {
  pick: ModelPick | null;
  request: (modelId: string, modality: Modality) => void;
}

export const useModelPick = create<ModelPickStore>((set) => ({
  pick: null,
  request: (modelId, modality) =>
    set((state) => ({
      pick: {
        modelId,
        modality,
        nonce: (state.pick?.nonce ?? 0) + 1,
      },
    })),
}));

export const useRestore = create<RestoreStore>((set) => ({
  request: null,
  publish: (payload) =>
    set((state) => ({
      request: { ...payload, nonce: (state.request?.nonce ?? 0) + 1 },
    })),
  clear: () => set({ request: null }),
}));
