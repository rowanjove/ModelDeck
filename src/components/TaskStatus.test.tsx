import { render, screen } from "@testing-library/react";
import { expect, it } from "vitest";
import type { TaskEvent } from "../types";
import { TaskStatus } from "./TaskStatus";

function task(eventType: TaskEvent["eventType"], progress?: number): TaskEvent {
  return {
    taskId: "preview",
    eventType,
    status: eventType,
    modality: "image",
    progress,
    timestamp: "2026-10-02",
  };
}

it("announces queue and terminal states without treating cancellation as failure", () => {
  const { rerender } = render(<TaskStatus task={task("queued")} />);
  expect(screen.getByRole("status")).toHaveTextContent("排队中");
  rerender(<TaskStatus task={task("cancelled")} />);
  expect(screen.getByRole("status")).toHaveTextContent("已取消");
  expect(screen.queryByRole("progressbar")).not.toBeInTheDocument();
  rerender(<TaskStatus task={task("completed")} />);
  expect(screen.getByRole("status")).toHaveTextContent("已完成");
});

it("shows zero progress and clamps malformed progress to the supported range", () => {
  const { rerender } = render(<TaskStatus task={task("progress", 0)} />);
  expect(screen.getByRole("progressbar")).toHaveAttribute("value", "0");
  rerender(<TaskStatus task={task("progress", 1.5)} />);
  expect(screen.getByRole("progressbar")).toHaveAttribute("value", "100");
  rerender(<TaskStatus task={task("progress", Number.NaN)} />);
  expect(screen.queryByRole("progressbar")).not.toBeInTheDocument();
});
