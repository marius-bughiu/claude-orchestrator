import { create } from "zustand";
import * as api from "../api";
import { notify } from "../lib/notify";
import type {
  OrchestratorEvent,
  OrchestratorStatus,
  Project,
  ScheduledTask,
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
  scheduled: ScheduledTask[];
  logs: LogLine[];

  init: () => Promise<void>;
  refreshStatus: () => Promise<void>;
  refreshProjects: () => Promise<void>;
  refreshTasks: () => Promise<void>;
  refreshTimeline: () => Promise<void>;
  refreshSettings: () => Promise<void>;
  refreshScheduled: () => Promise<void>;
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
  scheduled: [],
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
  refreshScheduled: async () => set({ scheduled: await api.listScheduled() }),

  refreshAll: async () => {
    const [status, projects, tasks, timeline, settings, scheduled] = await Promise.all([
      api.getStatus(),
      api.listProjects(),
      api.listTasks(),
      api.getTimeline(200),
      api.getSettings(),
      api.listScheduled(),
    ]);
    set({ status, projects, tasks, timeline, settings, scheduled });
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
        const prev = idx >= 0 ? tasks[idx] : undefined;
        if (idx >= 0) tasks[idx] = event.task;
        else tasks.unshift(event.task);
        set({ tasks });
        // Notify on a real completion/failure transition.
        const t = event.task;
        const becameDone = (t.status === "completed" || t.status === "failed") && prev?.status !== t.status;
        if (becameDone && get().settings?.notificationsEnabled !== false) {
          notify(t.status === "completed" ? "Task completed" : "Task failed", t.title);
        }
        break;
      }
      case "sessionUpdated":
        // Session lifecycle affects the timeline and status counters.
        get().refreshTimeline();
        get().refreshStatus();
        break;
      case "scheduledChanged":
        get().refreshScheduled();
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
