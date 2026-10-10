/**
 * 视频工具函数：从视频中提取指定时间点（默认最后一帧）为图片 Data URL
 */
export async function captureVideoFrame(
  videoSource: string,
  timeFraction = 0.98,
): Promise<string> {
  return new Promise((resolve, reject) => {
    const video = document.createElement("video");
    video.crossOrigin = "anonymous";
    video.muted = true;
    video.preload = "auto";
    video.src = videoSource;

    const timeout = setTimeout(() => {
      cleanup();
      reject(new Error("视频读取或加载帧超时"));
    }, 15000);

    function cleanup() {
      clearTimeout(timeout);
      video.onloadedmetadata = null;
      video.onseeked = null;
      video.onerror = null;
      video.pause();
      video.src = "";
      video.load();
    }

    video.onloadedmetadata = () => {
      const duration =
        Number.isFinite(video.duration) && video.duration > 0
          ? video.duration
          : 1;
      const targetTime = Math.max(
        0,
        Math.min(duration, duration * timeFraction),
      );
      video.currentTime = targetTime;
    };

    video.onseeked = () => {
      try {
        const canvas = document.createElement("canvas");
        canvas.width = video.videoWidth || 1280;
        canvas.height = video.videoHeight || 720;
        const ctx = canvas.getContext("2d");
        if (!ctx) {
          cleanup();
          reject(new Error("无法创建 Canvas 绘制上下文"));
          return;
        }
        ctx.drawImage(video, 0, 0, canvas.width, canvas.height);
        const dataUrl = canvas.toDataURL("image/png");
        cleanup();
        resolve(dataUrl);
      } catch (err) {
        cleanup();
        reject(err instanceof Error ? err : new Error(String(err)));
      }
    };

    video.onerror = () => {
      cleanup();
      reject(new Error("视频文件无法解析或格式不支持"));
    };
  });
}
