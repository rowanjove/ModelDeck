import { convertFileSrc } from "@tauri-apps/api/core";

export function AudioThumb({ path }: { path: string }) {
  if (!path) {
    return <p className="muted">无音频</p>;
  }
  const src =
    path.startsWith("data:") ||
    path.startsWith("http://") ||
    path.startsWith("https://")
      ? path
      : convertFileSrc(path);

  return <audio src={src} controls preload="metadata" />;
}
