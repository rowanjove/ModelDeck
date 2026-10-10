import { useState } from "react";
import type { ParameterDefinition } from "../../types";

export interface ShotPresetValues {
  camera: string;
  motion: string;
  shot: string;
  duration?: number;
  fps?: number;
  resolution?: string;
}

interface ShotPresetBarProps {
  modelParameters?: ParameterDefinition[];
  currentValues: Record<string, unknown>;
  onApplyParameters: (patch: Record<string, unknown>) => void;
  onAppendPrompt: (tag: string) => void;
}

const SHOT_PRESETS = [
  {
    name: " 电影推镜头",
    camera: "Dolly In",
    motion: "Smooth",
    shot: "Medium Shot",
    tag: ", cinematic dolly in push, medium shot, smooth cinematic motion",
  },
  {
    name: " 史诗全景环绕",
    camera: "Orbit 360",
    motion: "Smooth",
    shot: "Wide Shot",
    tag: ", epic 360 orbit rotation camera, panoramic wide angle shot",
  },
  {
    name: " 细腻特写微动",
    camera: "Static",
    motion: "Subtle",
    shot: "Close-Up",
    tag: ", close-up shot, static camera with subtle organic breathing motion, high detail",
  },
  {
    name: " 航拍俯冲全景",
    camera: "Crane Down",
    motion: "Dynamic",
    shot: "Extreme Long Shot",
    tag: ", dynamic aerial drone crane shot descending, majestic wide landscape view",
  },
  {
    name: " 动感手持跟随",
    camera: "Handheld Follow",
    motion: "Dynamic",
    shot: "Medium Shot",
    tag: ", dynamic handheld camera tracking motion, slight realistic camera shake",
  },
];

export function ShotPresetBar({
  modelParameters = [],
  currentValues: _currentValues,
  onApplyParameters,
  onAppendPrompt,
}: ShotPresetBarProps) {
  const [camera, setCamera] = useState<string>("");
  const [motion, setMotion] = useState<string>("");
  const [shot, setShot] = useState<string>("");

  const hasCameraParam = modelParameters.some((p) =>
    p.key.toLowerCase().includes("camera"),
  );
  const hasMotionParam = modelParameters.some(
    (p) =>
      p.key.toLowerCase().includes("motion") ||
      p.key.toLowerCase().includes("cfg_scale"),
  );
  const hasShotParam = modelParameters.some(
    (p) =>
      p.key.toLowerCase().includes("shot") ||
      p.key.toLowerCase().includes("aspect"),
  );

  function applyPreset(preset: (typeof SHOT_PRESETS)[0]) {
    setCamera(preset.camera);
    setMotion(preset.motion);
    setShot(preset.shot);

    const patch: Record<string, unknown> = {};
    if (hasCameraParam) {
      const p = modelParameters.find((item) =>
        item.key.toLowerCase().includes("camera"),
      );
      if (p) patch[p.key] = preset.camera;
    }
    if (hasMotionParam) {
      const p = modelParameters.find((item) =>
        item.key.toLowerCase().includes("motion"),
      );
      if (p) patch[p.key] = preset.motion;
    }
    if (hasShotParam) {
      const p = modelParameters.find((item) =>
        item.key.toLowerCase().includes("shot"),
      );
      if (p) patch[p.key] = preset.shot;
    }

    if (Object.keys(patch).length > 0) {
      onApplyParameters(patch);
    }
    onAppendPrompt(preset.tag);
  }

  function handleCustomApply() {
    const parts: string[] = [];
    if (camera) parts.push(`${camera} camera`);
    if (shot) parts.push(`${shot}`);
    if (motion) parts.push(`${motion} motion`);

    if (parts.length > 0) {
      onAppendPrompt(`, ${parts.join(", ")}`);
    }
  }

  return (
    <div
      className="shot-preset-bar"
      style={{
        background: "var(--bg-secondary, #222)",
        padding: "8px 12px",
        borderRadius: "6px",
        border: "1px solid var(--border-color, #444)",
        marginBottom: "12px",
      }}
    >
      <div
        style={{
          display: "flex",
          justifyContent: "space-between",
          alignItems: "center",
          marginBottom: "6px",
        }}
      >
        <span
          style={{
            fontSize: "12px",
            fontWeight: 600,
            color: "var(--text-muted, #aaa)",
          }}
        >
          镜头预设 (Shot & Camera Presets)
        </span>
        <div style={{ display: "flex", gap: "6px" }}>
          {SHOT_PRESETS.map((preset) => (
            <button
              key={preset.name}
              type="button"
              className="button tiny secondary"
              style={{ fontSize: "11px", padding: "2px 6px" }}
              onClick={() => applyPreset(preset)}
              title={`点击应用：${preset.camera} · ${preset.shot} · ${preset.motion}`}
            >
              {preset.name}
            </button>
          ))}
        </div>
      </div>

      <div
        style={{
          display: "flex",
          gap: "8px",
          alignItems: "center",
          fontSize: "12px",
        }}
      >
        <label
          style={{
            display: "flex",
            alignItems: "center",
            gap: "4px",
            margin: 0,
          }}
        >
          <span style={{ color: "var(--text-muted, #888)" }}>运镜:</span>
          <select
            value={camera}
            onChange={(e) => setCamera(e.target.value)}
            style={{ fontSize: "11px", padding: "2px 4px" }}
          >
            <option value="">默认/自由</option>
            <option value="Static">固定机位 (Static)</option>
            <option value="Pan Left/Right">水平平移 (Pan)</option>
            <option value="Tilt Up/Down">垂直俯仰 (Tilt)</option>
            <option value="Dolly In">前进推镜 (Dolly In)</option>
            <option value="Dolly Out">后退拉镜 (Dolly Out)</option>
            <option value="Orbit 360">环绕旋转 (Orbit)</option>
            <option value="Crane Down">航拍升降 (Crane)</option>
            <option value="Handheld Follow">手持晃动 (Handheld)</option>
          </select>
        </label>

        <label
          style={{
            display: "flex",
            alignItems: "center",
            gap: "4px",
            margin: 0,
          }}
        >
          <span style={{ color: "var(--text-muted, #888)" }}>景别:</span>
          <select
            value={shot}
            onChange={(e) => setShot(e.target.value)}
            style={{ fontSize: "11px", padding: "2px 4px" }}
          >
            <option value="">默认/自由</option>
            <option value="Close-Up">特写 (Close-Up)</option>
            <option value="Medium Shot">中景 (Medium Shot)</option>
            <option value="Wide Shot">全景 (Wide Shot)</option>
            <option value="Extreme Long Shot">远景 (Long Shot)</option>
          </select>
        </label>

        <label
          style={{
            display: "flex",
            alignItems: "center",
            gap: "4px",
            margin: 0,
          }}
        >
          <span style={{ color: "var(--text-muted, #888)" }}>幅度:</span>
          <select
            value={motion}
            onChange={(e) => setMotion(e.target.value)}
            style={{ fontSize: "11px", padding: "2px 4px" }}
          >
            <option value="">默认/标准</option>
            <option value="Subtle">平缓微动 (Subtle)</option>
            <option value="Smooth">稳定平滑 (Smooth)</option>
            <option value="Dynamic">剧烈动感 (Dynamic)</option>
          </select>
        </label>

        {(camera || shot || motion) && (
          <button
            type="button"
            className="button tiny primary"
            style={{ padding: "2px 8px", fontSize: "11px", marginLeft: "auto" }}
            onClick={handleCustomApply}
          >
            + 注入到提示词
          </button>
        )}
      </div>
    </div>
  );
}
