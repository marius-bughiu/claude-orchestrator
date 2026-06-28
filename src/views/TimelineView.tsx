import { useEffect, useMemo } from "react";
import { Link } from "react-router-dom";
import clsx from "clsx";
import { useStore } from "../store";
import type { TimelineItem } from "../api/types";
import { AgentBadge, SessionKindBadge, SessionStatusBadge } from "../components/Badges";
import { formatCost, formatDuration, formatRelative } from "../lib/format";
import { EmptyState } from "../components/Modal";

function Row({ item }: { item: TimelineItem }) {
  const active = item.status === "running" || item.status === "pending";
  return (
    <Link
      to={`/sessions/${item.sessionId}`}
      className={clsx(
        "flex items-center gap-3 rounded-md border px-3 py-2.5 text-sm transition-colors hover:border-indigo-500/40",
        active
          ? "border-indigo-500/30 bg-indigo-600/5"
          : "border-[var(--color-border)] bg-[var(--color-surface)]",
      )}
    >
      <div className="flex w-44 shrink-0 items-center gap-1.5">
        <SessionStatusBadge status={item.status} />
      </div>
      <SessionKindBadge kind={item.kind} />
      <AgentBadge agent={item.agent} />
      <div className="min-w-0 flex-1">
        <div className="truncate text-neutral-200">
          {item.taskTitle ?? (item.kind === "roadmap" ? "Roadmap planning" : "Session")}
        </div>
        <div className="truncate text-xs text-neutral-500">{item.projectName}</div>
      </div>
      <div className="hidden w-24 shrink-0 text-right text-xs text-neutral-500 sm:block">
        {formatDuration(item.startedAt, item.endedAt)}
      </div>
      <div className="w-16 shrink-0 text-right text-xs text-neutral-500">{formatCost(item.costUsd)}</div>
      <div className="hidden w-20 shrink-0 text-right text-xs text-neutral-600 md:block">
        {formatRelative(item.startedAt ?? null)}
      </div>
    </Link>
  );
}

function ActivityLog() {
  const logs = useStore((s) => s.logs);
  if (logs.length === 0) return null;
  return (
    <div className="card mt-6 p-3">
      <h3 className="mb-2 text-xs font-semibold uppercase tracking-wide text-neutral-500">Activity</h3>
      <div className="flex max-h-56 flex-col gap-0.5 overflow-y-auto font-mono text-[11px]">
        {logs.map((l, i) => (
          <div key={i} className="flex gap-2">
            <span
              className={clsx(
                "shrink-0",
                l.level === "error" ? "text-red-400" : l.level === "warn" ? "text-amber-400" : "text-neutral-600",
              )}
            >
              [{l.level}]
            </span>
            <span className="text-neutral-400">{l.message}</span>
          </div>
        ))}
      </div>
    </div>
  );
}

export function TimelineView() {
  const timeline = useStore((s) => s.timeline);
  const refreshTimeline = useStore((s) => s.refreshTimeline);

  useEffect(() => {
    refreshTimeline();
  }, [refreshTimeline]);

  const { inProgress, completed } = useMemo(() => {
    const inProgress = timeline.filter((t) => t.status === "running" || t.status === "pending");
    const completed = timeline.filter((t) => t.status !== "running" && t.status !== "pending");
    return { inProgress, completed };
  }, [timeline]);

  return (
    <div className="p-6">
      <div className="mb-5">
        <h1 className="text-lg font-semibold text-neutral-100">Timeline</h1>
        <p className="text-xs text-neutral-500">Sessions in progress and recently completed across all projects.</p>
      </div>

      {timeline.length === 0 ? (
        <EmptyState title="Nothing has run yet" hint="Start the orchestrator and add tasks to see activity here." />
      ) : (
        <>
          {inProgress.length > 0 && (
            <div className="mb-6">
              <h3 className="mb-2 text-xs font-semibold uppercase tracking-wide text-indigo-300">
                In progress · {inProgress.length}
              </h3>
              <div className="flex flex-col gap-1.5">
                {inProgress.map((t) => <Row key={t.sessionId} item={t} />)}
              </div>
            </div>
          )}
          <h3 className="mb-2 text-xs font-semibold uppercase tracking-wide text-neutral-500">
            History · {completed.length}
          </h3>
          <div className="flex flex-col gap-1.5">
            {completed.map((t) => <Row key={t.sessionId} item={t} />)}
          </div>
        </>
      )}

      <ActivityLog />
    </div>
  );
}
