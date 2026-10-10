import { convertFileSrc } from "@tauri-apps/api/core";
import { useEffect, useRef, useState } from "react";

export function MaskEditor({
  sourcePath,
  onPaint,
}: {
  sourcePath: string;
  onPaint: (base64: string | null) => void;
}) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const baseCanvasRef = useRef<HTMLCanvasElement | null>(null);
  const maskCanvasRef = useRef<HTMLCanvasElement | null>(null);
  const lastPointRef = useRef<{ x: number; y: number } | null>(null);
  const hasDrawnRef = useRef(false);
  const drawing = useRef(false);
  const [dimensions, setDimensions] = useState<{
    width: number;
    height: number;
  } | null>(null);
  const [zoom, setZoom] = useState(1);
  const [tool, setTool] = useState<"brush" | "erase">("brush");
  const [radius, setRadius] = useState(18);
  const [ready, setReady] = useState(false);
  const [failed, setFailed] = useState(false);

  useEffect(() => {
    let cancelled = false;
    setReady(false);
    setFailed(false);
    setDimensions(null);
    hasDrawnRef.current = false;
    baseCanvasRef.current = null;
    maskCanvasRef.current = null;

    const image = new Image();
    image.crossOrigin = "anonymous";
    image.onload = () => {
      if (cancelled) {
        return;
      }
      const w = image.width;
      const h = image.height;
      setDimensions({ width: w, height: h });

      // 1. 底图离屏 Canvas
      const baseCanvas = document.createElement("canvas");
      baseCanvas.width = w;
      baseCanvas.height = h;
      const baseCtx = baseCanvas.getContext("2d");
      if (baseCtx) {
        baseCtx.drawImage(image, 0, 0);
      }
      baseCanvasRef.current = baseCanvas;

      // 2. 遮罩离屏 Canvas
      const maskCanvas = document.createElement("canvas");
      maskCanvas.width = w;
      maskCanvas.height = h;
      maskCanvasRef.current = maskCanvas;

      // 3. 主显示 Canvas
      const canvas = canvasRef.current;
      if (canvas) {
        canvas.width = w;
        canvas.height = h;
        const ctx = canvas.getContext("2d");
        if (ctx) {
          ctx.drawImage(baseCanvas, 0, 0);
        }
      }
      setReady(true);
    };
    image.onerror = () => {
      if (!cancelled) {
        setFailed(true);
      }
    };
    image.src = convertFileSrc(sourcePath);

    return () => {
      cancelled = true;
    };
  }, [sourcePath]);

  function renderView() {
    const canvas = canvasRef.current;
    const baseCanvas = baseCanvasRef.current;
    const maskCanvas = maskCanvasRef.current;
    if (!canvas || !baseCanvas || !maskCanvas) {
      return;
    }
    const ctx = canvas.getContext("2d");
    if (!ctx) {
      return;
    }
    ctx.clearRect(0, 0, canvas.width, canvas.height);
    ctx.drawImage(baseCanvas, 0, 0);
    ctx.drawImage(maskCanvas, 0, 0);
  }

  function drawStroke(
    from: { x: number; y: number } | null,
    to: { x: number; y: number },
  ) {
    const maskCanvas = maskCanvasRef.current;
    if (!maskCanvas) {
      return;
    }
    const ctx = maskCanvas.getContext("2d");
    if (!ctx) {
      return;
    }

    ctx.save();
    if (tool === "brush") {
      ctx.globalCompositeOperation = "source-over";
      ctx.strokeStyle = "rgba(196, 42, 42, 0.65)";
      ctx.fillStyle = "rgba(196, 42, 42, 0.65)";
    } else {
      ctx.globalCompositeOperation = "destination-out";
      ctx.strokeStyle = "rgba(0, 0, 0, 1)";
      ctx.fillStyle = "rgba(0, 0, 0, 1)";
    }
    ctx.lineWidth = radius * 2;
    ctx.lineCap = "round";
    ctx.lineJoin = "round";

    ctx.beginPath();
    if (from) {
      ctx.moveTo(from.x, from.y);
      ctx.lineTo(to.x, to.y);
      ctx.stroke();
    } else {
      ctx.arc(to.x, to.y, radius, 0, Math.PI * 2);
      ctx.fill();
    }
    ctx.restore();

    hasDrawnRef.current = true;
    renderView();
  }

  function exportMask(): string | null {
    const maskCanvas = maskCanvasRef.current;
    if (!maskCanvas || !hasDrawnRef.current) {
      return null;
    }
    const w = maskCanvas.width;
    const h = maskCanvas.height;

    const exportCanvas = document.createElement("canvas");
    exportCanvas.width = w;
    exportCanvas.height = h;
    const expCtx = exportCanvas.getContext("2d");
    if (!expCtx) {
      return null;
    }

    expCtx.fillStyle = "#ffffff";
    expCtx.fillRect(0, 0, w, h);

    expCtx.globalCompositeOperation = "destination-out";
    expCtx.drawImage(maskCanvas, 0, 0);

    const url = exportCanvas.toDataURL("image/png");
    const comma = url.indexOf(",");
    return comma >= 0 ? url.slice(comma + 1) : null;
  }

  function point(event: React.PointerEvent<HTMLCanvasElement>) {
    const canvas = canvasRef.current;
    if (!canvas) {
      return null;
    }
    const rect = canvas.getBoundingClientRect();
    if (rect.width === 0 || rect.height === 0) {
      return null;
    }
    return {
      x: ((event.clientX - rect.left) / rect.width) * canvas.width,
      y: ((event.clientY - rect.top) / rect.height) * canvas.height,
    };
  }

  if (failed) {
    return <p className="muted">原图无法读取，蒙版编辑不可用。</p>;
  }

  return (
    <div className="mask-editor">
      <div
        className="mask-bar"
        style={{
          display: "flex",
          flexWrap: "wrap",
          alignItems: "center",
          gap: "8px",
        }}
      >
        <button
          type="button"
          className={tool === "brush" ? "primary" : undefined}
          onClick={() => setTool("brush")}
        >
          画笔
        </button>
        <button
          type="button"
          className={tool === "erase" ? "primary" : undefined}
          onClick={() => setTool("erase")}
        >
          橡皮
        </button>
        <button
          type="button"
          onClick={() => {
            const maskCanvas = maskCanvasRef.current;
            if (!maskCanvas) return;
            const ctx = maskCanvas.getContext("2d");
            if (!ctx) return;
            ctx.fillStyle = "rgba(196, 42, 42, 0.65)";
            ctx.fillRect(0, 0, maskCanvas.width, maskCanvas.height);
            hasDrawnRef.current = true;
            renderView();
            onPaint(exportMask());
          }}
          disabled={!ready}
        >
          全选
        </button>
        <button
          type="button"
          onClick={() => {
            const maskCanvas = maskCanvasRef.current;
            if (maskCanvas) {
              const ctx = maskCanvas.getContext("2d");
              ctx?.clearRect(0, 0, maskCanvas.width, maskCanvas.height);
            }
            hasDrawnRef.current = false;
            renderView();
            onPaint(null);
          }}
          disabled={!ready}
        >
          清空
        </button>
        <button
          type="button"
          onClick={() => {
            const maskCanvas = maskCanvasRef.current;
            if (!maskCanvas) {
              return;
            }
            const ctx = maskCanvas.getContext("2d");
            if (!ctx) {
              return;
            }
            const temp = document.createElement("canvas");
            temp.width = maskCanvas.width;
            temp.height = maskCanvas.height;
            const tctx = temp.getContext("2d");
            if (!tctx) {
              return;
            }
            tctx.fillStyle = "rgba(196, 42, 42, 0.65)";
            tctx.fillRect(0, 0, temp.width, temp.height);
            tctx.globalCompositeOperation = "destination-out";
            tctx.drawImage(maskCanvas, 0, 0);

            ctx.clearRect(0, 0, maskCanvas.width, maskCanvas.height);
            ctx.drawImage(temp, 0, 0);
            hasDrawnRef.current = true;
            renderView();
            onPaint(exportMask());
          }}
          disabled={!ready}
        >
          反相
        </button>
        <label style={{ display: "flex", alignItems: "center", gap: "6px" }}>
          笔刷
          <input
            type="range"
            min={4}
            max={72}
            value={radius}
            onChange={(event) => setRadius(Number(event.target.value))}
          />
        </label>
        <div style={{ display: "flex", alignItems: "center", gap: "4px" }}>
          <button
            type="button"
            onClick={() => setZoom((z) => Math.max(0.5, z - 0.25))}
            title="缩小"
          >
            -
          </button>
          <span
            style={{ fontSize: "12px", minWidth: "36px", textAlign: "center" }}
          >
            {Math.round(zoom * 100)}%
          </span>
          <button
            type="button"
            onClick={() => setZoom((z) => Math.min(2.5, z + 0.25))}
            title="放大"
          >
            +
          </button>
        </div>
        {dimensions && (
          <span
            className="muted"
            style={{ fontSize: "12px", marginLeft: "auto" }}
          >
            {dimensions.width} × {dimensions.height}
          </span>
        )}
      </div>
      <p className="muted">红色区域会被编辑，透明像素表示要修改的地方。</p>
      <div
        className="mask-stage"
        style={{ overflow: "auto", maxHeight: "480px" }}
      >
        <div
          style={{
            transform: `scale(${zoom})`,
            transformOrigin: "top left",
            transition: "transform 0.12s ease",
          }}
        >
          <canvas
            ref={canvasRef}
            onPointerDown={(event) => {
              const next = point(event);
              if (!next) {
                return;
              }
              drawing.current = true;
              lastPointRef.current = next;
              event.currentTarget.setPointerCapture(event.pointerId);
              drawStroke(null, next);
            }}
            onPointerMove={(event) => {
              if (!drawing.current) {
                return;
              }
              const next = point(event);
              if (!next) {
                return;
              }
              drawStroke(lastPointRef.current, next);
              lastPointRef.current = next;
            }}
            onPointerUp={(event) => {
              if (!drawing.current) {
                return;
              }
              drawing.current = false;
              lastPointRef.current = null;
              event.currentTarget.releasePointerCapture(event.pointerId);
              onPaint(exportMask());
            }}
          />
        </div>
      </div>
    </div>
  );
}
