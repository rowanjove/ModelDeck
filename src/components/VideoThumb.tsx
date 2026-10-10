import { convertFileSrc } from "@tauri-apps/api/core";

export function VideoThumb({ path }: { path: string }) {
  if (!path) {
    return <p className="muted">无视频</p>;
  }
  const src =
    path.startsWith("data:") ||
    path.startsWith("http://") ||
    path.startsWith("https://")
      ? path
      : convertFileSrc(path);

  if (path.toLowerCase().endsWith(".gif")) {
    return <img src={src} alt="生成视频" style={{ maxWidth: "100%" }} />;
  }

  return (
    <video src={src} controls preload="metadata" style={{ maxWidth: "100%" }} />
  );
}
