import { open } from "@tauri-apps/plugin-dialog";
import type { MediaKind } from "../types";

const EXTENSIONS: Record<MediaKind, string[]> = {
  image: ["png", "jpg", "jpeg", "webp", "gif"],
  video: ["mp4", "webm", "mov"],
  audio: ["wav", "mp3", "ogg", "flac", "m4a"],
};

export async function chooseFiles(
  kind: MediaKind,
  multiple: boolean,
): Promise<string[]> {
  const selected = await open({
    multiple,
    directory: false,
    filters: [{ name: kind, extensions: EXTENSIONS[kind] }],
  });
  if (selected == null) {
    return [];
  }
  return Array.isArray(selected) ? selected : [selected];
}

export function mediaKindOf(type: string): MediaKind {
  if (type.startsWith("video")) {
    return "video";
  }
  if (type.startsWith("audio")) {
    return "audio";
  }
  return "image";
}

export function isMultipleMedia(type: string): boolean {
  return type === "images" || type === "videos" || type === "audios";
}
