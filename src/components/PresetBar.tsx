import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { api, asAppError } from "../api";
import type { AppError } from "../types";
import { ErrorNote } from "./ErrorNote";

export function PresetBar({
  modelId,
  values,
  systemPrompt,
  userTemplate,
  onApply,
  onApplyPreset,
}: {
  modelId: string;
  values: Record<string, unknown>;
  systemPrompt?: string;
  userTemplate?: string;
  onApply: (values: Record<string, unknown>) => void;
  onApplyPreset?: (preset: import("../types").Preset) => void;
}) {
  const queryClient = useQueryClient();
  const presets = useQuery({
    queryKey: ["presets", modelId],
    queryFn: () => api.presets(modelId),
  });
  const [name, setName] = useState("");
  const [selected, setSelected] = useState("");
  const [includePrompts, setIncludePrompts] = useState(true);
  const [error, setError] = useState<AppError | null>(null);
  const refresh = () =>
    queryClient.invalidateQueries({ queryKey: ["presets", modelId] });
  const save = useMutation({
    mutationFn: api.createPreset,
    onSuccess: async (preset) => {
      setName("");
      setSelected(preset.id);
      setError(null);
      await refresh();
    },
    onError: (reason) => setError(asAppError(reason)),
  });
  const remove = useMutation({
    mutationFn: api.deletePreset,
    onSuccess: async () => {
      setSelected("");
      setError(null);
      await refresh();
    },
    onError: (reason) => setError(asAppError(reason)),
  });
  const items = presets.data ?? [];
  const current = items.find((item) => item.id === selected);

  const hasPrompts = Boolean(systemPrompt?.trim() || userTemplate?.trim());

  return (
    <div className="preset-bar">
      <p className="eyebrow">预设</p>
      <div className="preset-row">
        <select
          aria-label="参数预设"
          value={items.some((item) => item.id === selected) ? selected : ""}
          onChange={(event) => setSelected(event.target.value)}
        >
          <option value="">选择预设</option>
          {items.map((item) => (
            <option key={item.id} value={item.id}>
              {item.name}
              {item.systemPrompt || item.userTemplate ? " (含提示词)" : ""}
            </option>
          ))}
        </select>
        <button
          type="button"
          disabled={!current}
          onClick={() => {
            if (!current || !isRecord(current.parameters)) {
              return;
            }
            onApply(current.parameters);
            if (onApplyPreset) {
              onApplyPreset(current);
            }
          }}
        >
          应用
        </button>
        <button
          type="button"
          disabled={!current || remove.isPending}
          onClick={() => {
            if (!current) {
              return;
            }
            if (window.confirm(`删除预设「${current.name}」？`)) {
              remove.mutate(current.id);
            }
          }}
        >
          删除
        </button>
      </div>
      <div className="preset-row">
        <input
          aria-label="预设名称"
          value={name}
          placeholder="预设名称"
          onChange={(event) => setName(event.target.value)}
        />
        <button
          type="button"
          disabled={save.isPending}
          onClick={() => {
            const trimmed = name.trim();
            if (!trimmed) {
              setError({ code: "ValidationError", message: "请填写预设名称" });
              return;
            }
            save.mutate({
              name: trimmed,
              modelId,
              parameters: values,
              systemPrompt:
                includePrompts && systemPrompt?.trim()
                  ? systemPrompt.trim()
                  : undefined,
              userTemplate:
                includePrompts && userTemplate?.trim()
                  ? userTemplate.trim()
                  : undefined,
            });
          }}
        >
          保存预设
        </button>
      </div>
      {hasPrompts && (
        <label
          style={{
            display: "flex",
            alignItems: "center",
            gap: "6px",
            fontSize: "12px",
            color: "var(--muted)",
            cursor: "pointer",
            marginTop: "2px",
          }}
        >
          <input
            type="checkbox"
            checked={includePrompts}
            onChange={(e) => setIncludePrompts(e.target.checked)}
          />
          包含当前提示词 / 模板
        </label>
      )}
      {presets.error ? <ErrorNote error={asAppError(presets.error)} /> : null}
      {error ? <ErrorNote error={error} /> : null}
    </div>
  );
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return Boolean(value) && typeof value === "object" && !Array.isArray(value);
}
