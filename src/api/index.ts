// Thin typed wrappers around Tauri `invoke` plus the event subscription.

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  AddProjectInput,
  CreateTaskInput,
  OrchestratorEvent,
  OrchestratorStatus,
  Project,
  Session,
  SessionEvent,
  Settings,
  Task,
  TimelineItem,
} from "./types";

export const EVENT_CHANNEL = "orchestrator://event";

// ---- Projects --------------------------------------------------------------
export const listProjects = () => invoke<Project[]>("list_projects");
export const getProject = (id: string) => invoke<Project>("get_project", { id });
export const addProject = (input: AddProjectInput) =>
  invoke<Project>("add_project", { input });
export const updateProject = (project: Project) =>
  invoke<void>("update_project", { project });
export const removeProject = (id: string) => invoke<void>("remove_project", { id });
export const scaffoldProject = (id: string) =>
  invoke<string[]>("scaffold_project", { id });
export const projectConventions = (id: string) =>
  invoke<boolean>("project_conventions", { id });

// ---- Tasks -----------------------------------------------------------------
export const listTasks = (projectId?: string) =>
  invoke<Task[]>("list_tasks", { projectId: projectId ?? null });
export const getTask = (id: string) => invoke<Task>("get_task", { id });
export const createTask = (input: CreateTaskInput) =>
  invoke<Task>("create_task", { input });
export const updateTask = (task: Task) => invoke<void>("update_task", { task });
export const deleteTask = (id: string) => invoke<void>("delete_task", { id });
export const runTaskNow = (id: string) => invoke<void>("run_task_now", { id });

// ---- Sessions --------------------------------------------------------------
export const listSessions = (opts: { taskId?: string; projectId?: string } = {}) =>
  invoke<Session[]>("list_sessions", {
    taskId: opts.taskId ?? null,
    projectId: opts.projectId ?? null,
  });
export const getSession = (id: string) => invoke<Session>("get_session", { id });
export const getSessionEvents = (id: string) =>
  invoke<SessionEvent[]>("get_session_events", { id });
export const sendMessage = (sessionId: string, message: string) =>
  invoke<string>("send_message", { sessionId, message });
export const stopSession = (id: string) => invoke<void>("stop_session", { id });

// ---- Orchestrator ----------------------------------------------------------
export const getStatus = () => invoke<OrchestratorStatus>("get_status");
export const setRunning = (running: boolean) =>
  invoke<void>("set_running", { running });
export const getSettings = () => invoke<Settings>("get_settings");
export const updateSettings = (settings: Settings) =>
  invoke<void>("update_settings", { settings });
export const triggerRoadmap = (projectId: string) =>
  invoke<void>("trigger_roadmap", { projectId });
export const getTimeline = (limit?: number) =>
  invoke<TimelineItem[]>("get_timeline", { limit: limit ?? null });

// ---- Events ----------------------------------------------------------------
export function onOrchestratorEvent(
  handler: (event: OrchestratorEvent) => void,
): Promise<UnlistenFn> {
  return listen<OrchestratorEvent>(EVENT_CHANNEL, (e) => handler(e.payload));
}
