import { listen } from "@tauri-apps/api/event";
import { create } from "zustand";
import { api } from "../api";
import type { TaskEvent } from "../types";

interface TaskQueueState {
  tasks: Record<string, TaskEvent>;
  activeIds: string[];
  initialized: boolean;
  upsertTask: (event: TaskEvent) => void;
  removeTask: (taskId: string) => void;
  cancelTask: (taskId: string) => Promise<boolean>;
}

const MAX_TERMINAL_TASKS = 100;

export const useTaskQueueStore = create<TaskQueueState>((set) => ({
  tasks: {},
  activeIds: [],
  initialized: false,
  upsertTask: (event: TaskEvent) => {
    set((state) => {
      const isTerminal = [
        "completed",
        "failed",
        "cancelled",
        "interrupted",
        "unknown_submission",
      ].includes(event.eventType);
      const activeIds = isTerminal
        ? state.activeIds.filter((id) => id !== event.taskId)
        : state.activeIds.includes(event.taskId)
          ? state.activeIds
          : [...state.activeIds, event.taskId];

      const updatedTasks = {
        ...state.tasks,
        [event.taskId]: event,
      };

      // 终态任务上限保护：最多缓存 MAX_TERMINAL_TASKS 条，超出按时间戳 FIFO 淘汰
      const terminalTaskIds = Object.keys(updatedTasks).filter((id) =>
        [
          "completed",
          "failed",
          "cancelled",
          "interrupted",
          "unknown_submission",
        ].includes(updatedTasks[id].eventType),
      );

      if (terminalTaskIds.length > MAX_TERMINAL_TASKS) {
        terminalTaskIds.sort((a, b) => {
          const tA = updatedTasks[a].timestamp || "";
          const tB = updatedTasks[b].timestamp || "";
          return tA.localeCompare(tB);
        });
        const toEvict = terminalTaskIds.slice(
          0,
          terminalTaskIds.length - MAX_TERMINAL_TASKS,
        );
        for (const id of toEvict) {
          delete updatedTasks[id];
        }
      }

      return {
        tasks: updatedTasks,
        activeIds,
      };
    });
  },
  removeTask: (taskId: string) => {
    set((state) => {
      const newTasks = { ...state.tasks };
      delete newTasks[taskId];
      return {
        tasks: newTasks,
        activeIds: state.activeIds.filter((id) => id !== taskId),
      };
    });
  },
  cancelTask: async (taskId: string) => {
    try {
      const res = await api.tasksCancel(taskId);
      return res;
    } catch {
      return false;
    }
  },
}));

let listenerUnsub: (() => void) | null = null;
let listenerInitialization: Promise<() => void> | null = null;

export async function initTaskQueueListener(): Promise<() => void> {
  if (listenerUnsub) {
    return listenerUnsub;
  }
  if (listenerInitialization) return listenerInitialization;

  listenerInitialization = (async () => {
    // 1. 先监听 Tauri Event 统一事件 "task:event"，避免初始化获取与监听建立之间发生事件漏单
    const unsub = await listen<TaskEvent>("task:event", (event) => {
      useTaskQueueStore.getState().upsertTask(event.payload);
    });

    listenerUnsub = unsub;
    useTaskQueueStore.setState({ initialized: true });

    // 2. 然后拉取正在运行的后台任务快照并安全合并
    try {
      const activeTasks = await api.tasksListActive();
      for (const t of activeTasks) {
        const current = useTaskQueueStore.getState().tasks[t.id];
        if (
          !current ||
          (t.updatedAt &&
            (!current.timestamp || t.updatedAt >= current.timestamp))
        ) {
          useTaskQueueStore.getState().upsertTask({
            taskId: t.id,
            eventType: (t.status as TaskEvent["eventType"]) || "started",
            modality: t.modality,
            status: t.status,
            progress: t.progress,
            stage: t.stage,
            message: t.message,
            outputAsset: t.output,
            error: t.error,
            timestamp: t.updatedAt,
          });
        }
      }
    } catch {
      // 忽略初始获取失败
    }

    return unsub;
  })();
  try {
    return await listenerInitialization;
  } catch (err) {
    listenerInitialization = null;
    throw err;
  }
}

export function useTaskState(taskId: string | null) {
  return useTaskQueueStore((state) =>
    taskId ? (state.tasks[taskId] ?? null) : null,
  );
}
