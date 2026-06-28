import { create } from "zustand";
import * as api from "../api";
import type {
  OrchestratorEvent,
  OrchestratorStatus,
  Project,
  Settings,
  Task,
  TimelineItem,
} from "../api/types";

export interface LogLine {
  level: string;
  message: string;
  ts: number;
}

interface StoreState {
  initialized: boolean;
  connected: boolean;
  status: OrchestratorStatus | null;
  projects: Project[];
  tasks: Task[];
  settings: Settings | null;
  timeline: TimelineItem[];
  logs: LogLine[];

  init: () => Promise<void>;
  refreshStatus: () => Promise<void>;
  refreshProjects: () => Promise<void>;
  refreshTasks: () => Promise<void>;
  refreshTimeline: () => Promise<void>;
  refreshSettings: () => Promise<void>;
  refreshAll: () => Promise<void>;
  handleEvent: (event: OrchestratorEvent) => void;
}

let unlisten: (() => void) | null = null;

export const useStore = create<StoreState>((set, get) => ({
  initialized: false,
  connected: false,
  status: null,
  projects: [],
  tasks: [],
  settings: null,
  timeline: [],
  logs: [],

  init: async () => {
    if (get().initialized) return;
    set({ initialized: true });
    try {
      await get().refreshAll();
      set({ connected: true });
    } catch (e) {
      console.error("init failed", e);
      set({ connected: false });
    }
    if (!unlisten) {
      unlisten = await api.onOrchestratorEvent((event) => get().handleEvent(event));
    }
  },

  refreshStatus: async () => set({ status: await api.getStatus() }),
  refreshProjects: async () => set({ projects: await api.listProjects() }),
  refreshTasks: async () => set({ tasks: await api.listTasks() }),
  refreshTimeline: async () => set({ timeline: await api.getTimeline(200) }),
  refreshSettings: async () => set({ settings: await api.getSettings() }),

  refreshAll: async () => {
    const [status, projects, tasks, timeline, settings] = await Promise.all([
      api.getStatus(),
      api.listProjects(),
      api.listTasks(),
      api.getTimeline(200),
      api.getSettings(),
    ]);
    set({ status, projects, tasks, timeline, settings });
  },

  handleEvent: (event: OrchestratorEvent) => {
    switch (event.type) {
      case "statusChanged":
      case "usageUpdated":
        get().refreshStatus();
        break;
      case "taskUpdated": {
        const tasks = get().tasks.slice();
        const idx = tasks.findIndex((t) => t.id === event.task.id);
        if (idx >= 0) tasks[idx] = event.task;
        else tasks.unshift(event.task);
        set({ tasks });
        break;
      }
      case "sessionUpdated":
        // Session lifecycle affects the timeline and status counters.
        get().refreshTimeline();
        get().refreshStatus();
        break;
      case "log": {
        const logs = [
          { level: event.level, message: event.message, ts: Date.now() },
          ...get().logs,
        ].slice(0, 200);
        set({ logs });
        break;
      }
    }
  },
}));
