import type { AgentKind, SessionStatus, TaskStatus } from "../api/types";

export function formatCost(usd: number): string {
  if (!usd) return "$0.00";
  if (usd < 0.01) return "<$0.01";
  return `$${usd.toFixed(2)}`;
}

export function formatTokens(n: number): string {
  if (n < 1000) return `${n}`;
  if (n < 1_000_000) return `${(n / 1000).toFixed(n < 10_000 ? 1 : 0)}k`;
  return `${(n / 1_000_000).toFixed(1)}M`;
}

export function formatRelative(iso: string | null): string {
  if (!iso) return "—";
  const then = new Date(iso).getTime();
  const diff = Date.now() - then;
  if (Number.isNaN(then)) return "—";
  const s = Math.round(diff / 1000);
  if (s < 5) return "just now";
  if (s < 60) return `${s}s ago`;
  const m = Math.round(s / 60);
  if (m < 60) return `${m}m ago`;
  const h = Math.round(m / 60);
  if (h < 24) return `${h}h ago`;
  const d = Math.round(h / 24);
  return `${d}d ago`;
}

export function formatDuration(start: string | null, end: string | null): string {
  if (!start) return "—";
  const a = new Date(start).getTime();
  const b = end ? new Date(end).getTime() : Date.now();
  const s = Math.max(0, Math.round((b - a) / 1000));
  if (s < 60) return `${s}s`;
  const m = Math.floor(s / 60);
  const rem = s % 60;
  if (m < 60) return `${m}m ${rem}s`;
  const h = Math.floor(m / 60);
  return `${h}h ${m % 60}m`;
}

export const AGENT_LABELS: Record<AgentKind, string> = {
  claude: "Claude",
  gemini: "Gemini",
  codex: "Codex",
};

export const AGENT_COLORS: Record<AgentKind, string> = {
  claude: "border-orange-500/40 bg-orange-500/10 text-orange-300",
  gemini: "border-sky-500/40 bg-sky-500/10 text-sky-300",
  codex: "border-emerald-500/40 bg-emerald-500/10 text-emerald-300",
};

export const TASK_STATUS_META: Record<
  TaskStatus,
  { label: string; cls: string }
> = {
  pending: { label: "Pending", cls: "border-neutral-500/40 bg-neutral-500/10 text-neutral-300" },
  queued: { label: "Queued", cls: "border-amber-500/40 bg-amber-500/10 text-amber-300" },
  running: { label: "Running", cls: "border-indigo-500/40 bg-indigo-500/10 text-indigo-300" },
  needs_review: { label: "Needs review", cls: "border-yellow-500/40 bg-yellow-500/10 text-yellow-300" },
  completed: { label: "Completed", cls: "border-emerald-500/40 bg-emerald-500/10 text-emerald-300" },
  failed: { label: "Failed", cls: "border-red-500/40 bg-red-500/10 text-red-300" },
  cancelled: { label: "Cancelled", cls: "border-neutral-600/40 bg-neutral-600/10 text-neutral-400" },
  blocked: { label: "Blocked", cls: "border-purple-500/40 bg-purple-500/10 text-purple-300" },
};

export const SESSION_STATUS_META: Record<
  SessionStatus,
  { label: string; cls: string }
> = {
  pending: { label: "Pending", cls: "border-neutral-500/40 bg-neutral-500/10 text-neutral-300" },
  running: { label: "Running", cls: "border-indigo-500/40 bg-indigo-500/10 text-indigo-300" },
  completed: { label: "Completed", cls: "border-emerald-500/40 bg-emerald-500/10 text-emerald-300" },
  failed: { label: "Failed", cls: "border-red-500/40 bg-red-500/10 text-red-300" },
  cancelled: { label: "Cancelled", cls: "border-neutral-600/40 bg-neutral-600/10 text-neutral-400" },
  timed_out: { label: "Timed out", cls: "border-orange-500/40 bg-orange-500/10 text-orange-300" },
};

export function priorityLabel(priority: number): string {
  if (priority >= 200) return "Urgent";
  if (priority >= 100) return "High";
  if (priority >= 50) return "Normal";
  return "Low";
}
