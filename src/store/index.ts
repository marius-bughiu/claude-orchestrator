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

export interface ActivityItem {
  id: number;
  kind: "log" | "task" | "scheduled";
  level: string;
  message: string;
  ts: number;
}

let activitySeq = 1;

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
  activity: ActivityItem[];
  unread: number;
  /** Bumped to request the "New task" modal open (e.g. from the command palette). */
  newTaskNonce: number;
  /** Bumped to request the Settings view run diagnostics (from the palette). */
  diagnosticsNonce: number;

  init: () => Promise<void>;
  refreshStatus: () => Promise<void>;
  refreshProjects: () => Promise<void>;
  refreshTasks: () => Promise<void>;
  refreshTimeline: () => Promise<void>;
  refreshSettings: () => Promise<void>;
  refreshScheduled: () => Promise<void>;
  refreshAll: () => Promise<void>;
  markActivityRead: () => void;
  clearLogs: () => void;
  requestNewTask: () => void;
  requestDiagnostics: () => void;
  handleEvent: (event: OrchestratorEvent) => void;
}

function pushActivity(get: () => StoreState, set: (p: Partial<StoreState>) => void, item: Omit<ActivityItem, "id" | "ts">) {
  const entry: ActivityItem = { ...item, id: activitySeq++, ts: Date.now() };
  set({ activity: [entry, ...get().activity].slice(0, 100), unread: get().unread + 1 });
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
  activity: [],
  unread: 0,
  newTaskNonce: 0,
  diagnosticsNonce: 0,

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
  markActivityRead: () => set({ unread: 0 }),
  clearLogs: () => set({ logs: [] }),
  requestNewTask: () => set({ newTaskNonce: get().newTaskNonce + 1 }),
  requestDiagnostics: () => set({ diagnosticsNonce: get().diagnosticsNonce + 1 }),

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
        // Desktop notification on a real completion/failure transition. The
        // activity feed is fed by the separate `activity` event.
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
      case "activity":
        // Surface persisted activity in the in-memory feed / unread bell too.
        pushActivity(get, set, {
          kind: event.entry.kind === "scheduled" ? "scheduled" : event.entry.kind === "task" ? "task" : "log",
          level: event.entry.level,
          message: event.entry.message,
        });
        break;
      case "log": {
        const logs = [
          { level: event.level, message: event.message, ts: Date.now() },
          ...get().logs,
        ].slice(0, 200);
        set({ logs });
        pushActivity(get, set, {
          kind: event.message.toLowerCase().includes("scheduled") ? "scheduled" : "log",
          level: event.level,
          message: event.message,
        });
        break;
      }
    }
  },
}));
