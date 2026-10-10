import { fireEvent, render, screen } from "@testing-library/react";
import { expect, it, vi } from "vitest";
import type { ParameterDefinition } from "../types";
import { ParameterPanel } from "./ParameterPanel";

const parameters: ParameterDefinition[] = [
  {
    key: "temperature",
    label: "Temperature",
    type: "range",
    min: 0,
    max: 2,
    step: 0.1,
    default: 0.7,
  },
  {
    key: "format",
    label: "Format",
    type: "select",
    options: [
      { label: "Text", value: "text" },
      { label: "JSON", value: "json_object" },
    ],
  },
  { key: "enabled", label: "Enabled", type: "boolean" },
  { key: "stop", label: "Stop", type: "text" },
  { key: "seed", label: "Seed", type: "number", advanced: true },
  { key: "source", label: "Source", type: "image" },
  { key: "refs", label: "References", type: "images" },
];

it("keeps labels bound to their own controls when workspaces remain mounted", () => {
  const props = {
    parameters: parameters.slice(0, 1),
    constraints: [],
    values: { temperature: 0.7 },
    onChange: vi.fn(),
    chooseFiles: async () => [],
  };
  render(
    <>
      <ParameterPanel {...props} />
      <ParameterPanel {...props} />
    </>,
  );
  const sliders = screen.getAllByRole("slider");
  expect(sliders[0].id).not.toBe(sliders[1].id);
  for (const slider of sliders) {
    expect(
      document.querySelector(`label[for="${slider.id}"]`),
    ).toHaveTextContent("Temperature");
  }
});

it("renders controls from the parameter schema", () => {
  const onChange = vi.fn();
  render(
    <ParameterPanel
      parameters={parameters}
      constraints={[{ type: "mutuallyExclusive", fields: ["seed", "stop"] }]}
      values={{
        temperature: 0.4,
        stop: "END",
        seed: 1,
        source: "C:/images/source.png",
        refs: ["C:/images/a.png", "C:/images/b.png"],
      }}
      onChange={onChange}
      chooseFiles={async () => ["C:/images/c.png"]}
    />,
  );

  expect(screen.getByLabelText(/Temperature/)).toHaveAttribute("type", "range");
  expect(screen.getByLabelText("Format")).toBeInTheDocument();
  expect(screen.getByLabelText("Enabled")).toHaveAttribute("type", "checkbox");
  expect(screen.getByLabelText("Stop")).toHaveValue("END");
  expect(screen.getByLabelText("Seed")).toHaveAttribute("type", "number");
  expect(screen.getByText("source.png")).toBeInTheDocument();
  expect(screen.getByText("a.png")).toBeInTheDocument();
  expect(screen.getByText("b.png")).toBeInTheDocument();
  expect(screen.getByText(/不能同时设置/)).toBeInTheDocument();

  fireEvent.change(screen.getByLabelText(/Temperature/), {
    target: { value: "1.2" },
  });
  expect(onChange).toHaveBeenCalled();
  const next = onChange.mock.calls[0]?.[0] as Record<string, unknown>;
  expect(next.temperature).toBe(1.2);
});

it("respects conditional visibility and hides parameters when condition is unmet", () => {
  const onChange = vi.fn();
  const conditionalParams: ParameterDefinition[] = [
    {
      key: "style",
      label: "Style Mode",
      type: "select",
      options: [
        { label: "Anime", value: "anime" },
        { label: "Realistic", value: "realistic" },
      ],
      default: "anime",
    },
    {
      key: "detail_level",
      label: "Detail Level",
      type: "number",
    },
  ];

  // detail_level only visible when style == "realistic"
  const constraints = [
    {
      type: "conditionalVisibility" as const,
      field: "detail_level",
      dependsOn: "style",
      equals: "realistic",
    },
  ];

  const { rerender } = render(
    <ParameterPanel
      parameters={conditionalParams}
      constraints={constraints}
      values={{ style: "anime" }}
      onChange={onChange}
      chooseFiles={async () => []}
    />,
  );

  expect(screen.queryByLabelText("Detail Level")).toBeNull();

  rerender(
    <ParameterPanel
      parameters={conditionalParams}
      constraints={constraints}
      values={{ style: "realistic" }}
      onChange={onChange}
      chooseFiles={async () => []}
    />,
  );

  expect(screen.getByLabelText("Detail Level")).toBeInTheDocument();
});

it("renders group titles when parameters have group defined", () => {
  const onChange = vi.fn();
  const groupedParams: ParameterDefinition[] = [
    { key: "p1", label: "Param 1", type: "text", group: "生成控制" },
    { key: "p2", label: "Param 2", type: "number", group: "生成控制" },
    { key: "p3", label: "Param 3", type: "text", group: "后处理" },
  ];

  render(
    <ParameterPanel
      parameters={groupedParams}
      constraints={[]}
      values={{}}
      onChange={onChange}
      chooseFiles={async () => []}
    />,
  );

  expect(screen.getByText("生成控制")).toBeInTheDocument();
  expect(screen.getByText("后处理")).toBeInTheDocument();
});
