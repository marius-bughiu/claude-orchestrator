import { useEffect, useState } from "react";
import { Link } from "react-router-dom";
import { CalendarClock } from "lucide-react";
import * as api from "../api";
import type { UpcomingTask } from "../api/types";
import { AgentBadge } from "./Badges";
import { formatClock, formatUntil } from "../lib/format";

/// Shows the next N (default 10) projected firings of scheduled tasks. Rendered
/// in both the global and per-project task views. Hidden when there are none.
export function UpcomingTasks({
  projectId,
  showProject = false,
  limit = 10,
}: {
  projectId?: string;
  showProject?: boolean;
  limit?: number;
}) {
  const [items, setItems] = useState<UpcomingTask[]>([]);

  useEffect(() => {
    let active = true;
    const load = () =>
      api.upcomingTasks(projectId, limit).then((r) => active && setItems(r)).catch(() => {});
    load();
    // Refetch when scheduled tasks change (discovery / firing) and periodically
    // so the relative "in Xh" labels stay fresh.
    const unlisten = api.onOrchestratorEvent((e) => {
      if (e.type === "scheduledChanged") load();
    });
    const interval = setInterval(load, 60_000);
    return () => {
      active = false;
      clearInterval(interval);
      unlisten.then((u) => u());
    };
  }, [projectId, limit]);

  if (items.length === 0) return null;

  return (
    <div className="mb-6">
      <h3 className="mb-2 flex items-center gap-1.5 text-sm font-semibold text-neutral-200">
        <CalendarClock size={15} className="text-indigo-400" /> Upcoming
      </h3>
      <div className="overflow-hidden rounded-lg border border-[var(--color-border)]">
        {items.map((u, i) => (
          <div
            key={`${u.scheduledId}-${u.runAt}-${i}`}
            className="flex items-center gap-3 border-b border-[var(--color-border)] px-3 py-2 text-sm last:border-0"
          >
            <span className="w-16 shrink-0 text-xs font-medium text-indigo-300">
              {formatUntil(u.runAt)}
            </span>
            <span className="w-40 shrink-0 text-xs text-neutral-500">{formatClock(u.runAt)}</span>
            <span className="min-w-0 flex-1 truncate text-neutral-200">{u.title}</span>
            {showProject && (
              <Link
                to={`/projects/${u.projectId}`}
                className="shrink-0 text-xs text-neutral-400 hover:text-indigo-300"
              >
                {u.projectName}
              </Link>
            )}
            {u.agent && <AgentBadge agent={u.agent} />}
            <span className="hidden w-28 shrink-0 truncate text-right text-[11px] text-neutral-600 sm:block">
              {u.scheduleDesc}
            </span>
          </div>
        ))}
      </div>
    </div>
  );
}
