import { convertFileSrc } from "@tauri-apps/api/core";
import { useState } from "react";
import { fileName } from "../lib/schema";

export function ImageThumb({ path, alt }: { path: string; alt: string }) {
  const [failedPath, setFailedPath] = useState<string | null>(null);
  if (failedPath === path)
    return (
      <p className="muted" role="status">
        图像预览不可用，请检查原文件。
      </p>
    );
  if (!path) {
    return <p className="muted">无图像</p>;
  }
  const src =
    path.startsWith("data:") ||
    path.startsWith("http://") ||
    path.startsWith("https://")
      ? path
      : convertFileSrc(path);

  return (
    <img
      src={src}
      alt={alt || fileName(path)}
      loading="lazy"
      decoding="async"
      style={{ width: "100%", height: "auto", objectFit: "contain" }}
      onError={() => setFailedPath(path)}
    />
  );
}
