import { useId } from "react";
import { isMultipleMedia, mediaKindOf } from "../lib/files";
import {
  constraintMessages,
  isParameterVisible,
  withDefaults,
} from "../lib/schema";
import type { ConstraintDefinition, ParameterDefinition } from "../types";
import { FileList } from "./FileList";

function groupParameters(
  items: ParameterDefinition[],
): Array<{ name?: string; items: ParameterDefinition[] }> {
  const groups: Array<{ name?: string; items: ParameterDefinition[] }> = [];
  const map = new Map<string | undefined, ParameterDefinition[]>();
  for (const item of items) {
    const g = item.group?.trim() || undefined;
    if (!map.has(g)) {
      const list: ParameterDefinition[] = [];
      map.set(g, list);
      groups.push({ name: g, items: list });
    }
    map.get(g)?.push(item);
  }
  return groups;
}

export function ParameterPanel({
  parameters,
  constraints,
  values,
  onChange,
  chooseFiles,
}: {
  parameters: ParameterDefinition[];
  constraints: ConstraintDefinition[];
  values: Record<string, unknown>;
  onChange: (values: Record<string, unknown>) => void;
  chooseFiles: (
    kind: "image" | "video" | "audio",
    multiple: boolean,
  ) => Promise<string[]>;
}) {
  const visible = parameters.filter((parameter) =>
    isParameterVisible(parameter.key, constraints, values),
  );
  const core = visible.filter((parameter) => !parameter.advanced);
  const advanced = visible.filter((parameter) => parameter.advanced);
  const coreGroups = groupParameters(core);
  const advancedGroups = groupParameters(advanced);

  const messages = constraintMessages(
    constraints,
    withDefaults(parameters, values),
    parameters,
  );

  function update(key: string, value: unknown) {
    const next = { ...values };
    if (value === undefined || value === null || value === "") {
      delete next[key];
    } else {
      next[key] = value;
    }
    onChange(next);
  }

  return (
    <div className="parameter-panel">
      {parameters.length === 0 ? (
        <p className="muted">这个模型没有声明参数。</p>
      ) : null}
      {coreGroups.map((group, idx) => (
        <div
          key={group.name ?? `core-group-${idx}`}
          className="parameter-group"
        >
          {group.name ? (
            <h5 className="parameter-group-title">{group.name}</h5>
          ) : null}
          {group.items.map((parameter) => (
            <ParameterControl
              key={parameter.key}
              parameter={parameter}
              value={values[parameter.key]}
              onChange={(value) => update(parameter.key, value)}
              chooseFiles={chooseFiles}
            />
          ))}
        </div>
      ))}
      {advanced.length > 0 ? (
        <details className="fold">
          <summary>高级参数</summary>
          {advancedGroups.map((group, idx) => (
            <div
              key={group.name ?? `adv-group-${idx}`}
              className="parameter-group"
            >
              {group.name ? (
                <h5 className="parameter-group-title">{group.name}</h5>
              ) : null}
              {group.items.map((parameter) => (
                <ParameterControl
                  key={parameter.key}
                  parameter={parameter}
                  value={values[parameter.key]}
                  onChange={(value) => update(parameter.key, value)}
                  chooseFiles={chooseFiles}
                />
              ))}
            </div>
          ))}
        </details>
      ) : null}
      {messages.map((message) => (
        <p key={message} className="constraint">
          {message}
        </p>
      ))}
    </div>
  );
}

function ParameterControl({
  parameter,
  value,
  onChange,
  chooseFiles,
}: {
  parameter: ParameterDefinition;
  value: unknown;
  onChange: (value: unknown) => void;
  chooseFiles: (
    kind: "image" | "video" | "audio",
    multiple: boolean,
  ) => Promise<string[]>;
}) {
  const instanceId = useId();
  const id = `${instanceId}-param-${parameter.key}`;
  if (parameter.type === "boolean") {
    return (
      <label className="check" htmlFor={id}>
        <input
          id={id}
          type="checkbox"
          checked={value === true}
          onChange={(event) => onChange(event.target.checked)}
        />
        <span>{parameter.label}</span>
      </label>
    );
  }
  if (parameter.type === "range") {
    const numeric = typeof value === "number" ? value : (parameter.min ?? 0);
    return (
      <label className="field" htmlFor={id}>
        <span>
          {parameter.label}
          <em>{numeric}</em>
        </span>
        <input
          id={id}
          type="range"
          min={parameter.min}
          max={parameter.max}
          step={parameter.step ?? 1}
          value={numeric}
          onChange={(event) => onChange(Number(event.target.value))}
        />
      </label>
    );
  }
  if (parameter.type === "number") {
    return (
      <label className="field" htmlFor={id}>
        <span>{parameter.label}</span>
        <input
          id={id}
          type="number"
          min={parameter.min}
          max={parameter.max}
          step={parameter.step ?? 1}
          value={typeof value === "number" ? value : ""}
          onChange={(event) =>
            onChange(
              event.target.value === "" ? null : Number(event.target.value),
            )
          }
        />
      </label>
    );
  }
  if (parameter.type === "select") {
    const options = parameter.options ?? [];
    const selected = options.findIndex((option) => option.value === value);
    return (
      <label className="field" htmlFor={id}>
        <span>{parameter.label}</span>
        <select
          id={id}
          value={selected >= 0 ? String(selected) : ""}
          onChange={(event) => {
            if (event.target.value === "") {
              onChange(null);
              return;
            }
            onChange(options[Number(event.target.value)]?.value ?? null);
          }}
        >
          {parameter.required ? null : <option value="">不发送</option>}
          {options.map((option, index) => (
            <option key={`${parameter.key}-${option.label}`} value={index}>
              {option.label}
            </option>
          ))}
        </select>
      </label>
    );
  }
  if (
    parameter.type === "image" ||
    parameter.type === "images" ||
    parameter.type === "video" ||
    parameter.type === "videos" ||
    parameter.type === "audio" ||
    parameter.type === "audios"
  ) {
    const multiple = isMultipleMedia(parameter.type);
    const paths = multiple
      ? Array.isArray(value)
        ? value.filter((item): item is string => typeof item === "string")
        : []
      : typeof value === "string" && value
        ? [value]
        : [];
    return (
      <div className="field">
        <FileList
          label={parameter.label}
          paths={paths}
          multiple={multiple}
          onChoose={() => chooseFiles(mediaKindOf(parameter.type), multiple)}
          onChange={(next) => onChange(multiple ? next : (next[0] ?? null))}
        />
      </div>
    );
  }
  const multiline = parameter.type === "textarea";
  const text = typeof value === "string" ? value : "";
  return (
    <label className="field" htmlFor={id}>
      <span>{parameter.label}</span>
      {multiline ? (
        <textarea
          id={id}
          rows={4}
          value={text}
          onChange={(event) => onChange(event.target.value)}
        />
      ) : (
        <input
          id={id}
          type="text"
          value={text}
          onChange={(event) => onChange(event.target.value)}
        />
      )}
    </label>
  );
}
