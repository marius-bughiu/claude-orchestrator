import clsx from "clsx";
import type { AgentKind, SessionKind, SessionStatus, TaskStatus } from "../api/types";
import {
  AGENT_COLORS,
  AGENT_LABELS,
  priorityLabel,
  SESSION_STATUS_META,
  TASK_STATUS_META,
} from "../lib/format";

export function AgentBadge({ agent }: { agent: AgentKind }) {
  return <span className={clsx("chip", AGENT_COLORS[agent])}>{AGENT_LABELS[agent]}</span>;
}

export function TaskStatusBadge({ status }: { status: TaskStatus }) {
  const meta = TASK_STATUS_META[status];
  return <span className={clsx("chip", meta.cls)}>{meta.label}</span>;
}

export function SessionStatusBadge({ status }: { status: SessionStatus }) {
  const meta = SESSION_STATUS_META[status];
  const pulse = status === "running" || status === "pending";
  return (
    <span className={clsx("chip", meta.cls)}>
      {pulse && <span className="h-1.5 w-1.5 animate-pulse rounded-full bg-current" />}
      {meta.label}
    </span>
  );
}

const KIND_META: Record<SessionKind, { label: string; cls: string }> = {
  task: { label: "Task", cls: "border-neutral-600/50 bg-neutral-700/20 text-neutral-300" },
  roadmap: { label: "Roadmap", cls: "border-blue-500/40 bg-blue-500/10 text-blue-300" },
  verify: { label: "Verify", cls: "border-teal-500/40 bg-teal-500/10 text-teal-300" },
};

export function SessionKindBadge({ kind }: { kind: SessionKind }) {
  const meta = KIND_META[kind];
  return <span className={clsx("chip", meta.cls)}>{meta.label}</span>;
}

export function PriorityBadge({ priority }: { priority: number }) {
  const label = priorityLabel(priority);
  const cls =
    priority >= 200
      ? "border-red-500/40 bg-red-500/10 text-red-300"
      : priority >= 100
        ? "border-orange-500/40 bg-orange-500/10 text-orange-300"
        : priority >= 50
          ? "border-neutral-600/50 bg-neutral-700/20 text-neutral-300"
          : "border-neutral-700/50 bg-neutral-800/40 text-neutral-500";
  return <span className={clsx("chip", cls)}>{label}</span>;
}
