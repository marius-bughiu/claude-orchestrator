// Thin typed wrappers around Tauri `invoke` plus the event subscription.

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  AddProjectInput,
  CreateTaskInput,
  AgentStat,
  AgentHealth,
  ActivityEntry,
  BranchInfo,
  RebaseResult,
  OrchestratorEvent,
  OrchestratorStatus,
  GitStatus,
  Project,
  ProjectMemory,
  PullRequest,
  SessionDiff,
  ScheduledTask,
  UpcomingTask,
  UsagePoint,
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
export const projectGitStatus = (id: string) =>
  invoke<GitStatus>("project_git_status", { id });

// ---- Tasks -----------------------------------------------------------------
export const listTasks = (projectId?: string) =>
  invoke<Task[]>("list_tasks", { projectId: projectId ?? null });
export const getTask = (id: string) => invoke<Task>("get_task", { id });
export const createTask = (input: CreateTaskInput) =>
  invoke<Task>("create_task", { input });
export const createTasksBulk = (input: {
  projectId: string;
  text: string;
  priority?: number;
  agent?: string;
}) => invoke<Task[]>("create_tasks_bulk", { input });
export const updateTask = (task: Task) => invoke<void>("update_task", { task });
export const deleteTask = (id: string) => invoke<void>("delete_task", { id });
export const runTaskNow = (id: string) => invoke<void>("run_task_now", { id });
export const retryTask = (id: string) => invoke<void>("retry_task", { id });
export const cloneTask = (id: string) => invoke<Task>("clone_task", { id });

// ---- Sessions --------------------------------------------------------------
export const listSessions = (opts: { taskId?: string; projectId?: string } = {}) =>
  invoke<Session[]>("list_sessions", {
    taskId: opts.taskId ?? null,
    projectId: opts.projectId ?? null,
  });
export const getSession = (id: string) => invoke<Session>("get_session", { id });
export const getSessionEvents = (id: string) =>
  invoke<SessionEvent[]>("get_session_events", { id });
export const sendMessage = (sessionId: string, message: string, model?: string) =>
  invoke<string>("send_message", { sessionId, message, model: model ?? null });
/// Inject into a live session (or resume if finished). Returns the session id to show.
export const injectMessage = (sessionId: string, message: string, model?: string) =>
  invoke<string>("inject_message", { sessionId, message, model: model ?? null });
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
export const getActivity = (limit?: number, projectId?: string) =>
  invoke<ActivityEntry[]>("get_activity", { limit: limit ?? null, projectId: projectId ?? null });

// ---- Scheduled tasks -------------------------------------------------------
export const listScheduled = (projectId?: string) =>
  invoke<ScheduledTask[]>("list_scheduled", { projectId: projectId ?? null });
export const refreshScheduled = () => invoke<number>("refresh_scheduled");
export const setScheduledEnabled = (id: string, enabled: boolean) =>
  invoke<void>("set_scheduled_enabled", { id, enabled });
export const upcomingTasks = (projectId?: string, limit?: number) =>
  invoke<UpcomingTask[]>("upcoming_tasks", {
    projectId: projectId ?? null,
    limit: limit ?? null,
  });

// ---- Dashboards ------------------------------------------------------------
export const usageSeries = (
  granularity: "day" | "month" | "year",
  agent?: string,
  limit?: number,
) =>
  invoke<UsagePoint[]>("usage_series", {
    granularity,
    agent: agent ?? null,
    limit: limit ?? null,
  });

export const agentStats = () => invoke<AgentStat[]>("agent_stats");

// ---- Project memory --------------------------------------------------------
export const projectMemory = (id: string) =>
  invoke<ProjectMemory>("project_memory", { id });
export const generateProjectContext = (id: string) =>
  invoke<string>("generate_project_context", { id });

// ---- GitHub ----------------------------------------------------------------
export const importGithubIssues = (projectId: string) =>
  invoke<number>("import_github_issues", { projectId });
export const listPullRequests = (projectId: string) =>
  invoke<PullRequest[]>("list_pull_requests", { projectId });
export const mergePullRequest = (projectId: string, number: number) =>
  invoke<void>("merge_pull_request", { projectId, number });

// ---- Diffs -----------------------------------------------------------------
export const sessionDiff = (id: string) => invoke<SessionDiff>("session_diff", { id });

// ---- Agent health & maintenance --------------------------------------------
export const agentHealth = () => invoke<AgentHealth[]>("agent_health");
export const listBranches = (projectId: string) =>
  invoke<BranchInfo[]>("list_branches", { projectId });
export const deleteBranch = (projectId: string, branch: string) =>
  invoke<void>("delete_branch", { projectId, branch });
export const pruneWorktrees = (projectId: string) =>
  invoke<void>("prune_worktrees", { projectId });
export const rebaseBranch = (projectId: string, branch: string) =>
  invoke<RebaseResult>("rebase_branch", { projectId, branch });

// ---- Updates ---------------------------------------------------------------
export const beginDrain = () => invoke<void>("begin_drain");
export const cancelDrain = () => invoke<void>("cancel_drain");

// ---- Events ----------------------------------------------------------------
export function onOrchestratorEvent(
  handler: (event: OrchestratorEvent) => void,
): Promise<UnlistenFn> {
  return listen<OrchestratorEvent>(EVENT_CHANNEL, (e) => handler(e.payload));
}
