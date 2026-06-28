import { useEffect } from "react";
import { Link } from "react-router-dom";
import clsx from "clsx";
import { RefreshCw, Clock, AlertTriangle, FileText } from "lucide-react";
import { useStore } from "../store";
import * as api from "../api";
import type { ScheduledTask } from "../api/types";
import { AgentBadge } from "../components/Badges";
import { Switch } from "../components/Switch";
import { EmptyState } from "../components/Modal";

function nextRunLabel(iso: string | null): string {
  if (!iso) return "—";
  const then = new Date(iso).getTime();
  const diff = then - Date.now();
  if (Number.isNaN(then)) return "—";
  if (diff <= 0) return "due now";
  const m = Math.round(diff / 60000);
  if (m < 60) return `in ${m}m`;
  const h = Math.round(m / 60);
  if (h < 48) return `in ${h}h`;
  return `in ${Math.round(h / 24)}d`;
}

function Row({ item }: { item: ScheduledTask }) {
  const projects = useStore((s) => s.projects);
  const refreshScheduled = useStore((s) => s.refreshScheduled);
  const project = projects.find((p) => p.id === item.projectId);

  const toggle = async () => {
    await api.setScheduledEnabled(item.id, !item.enabled);
    await refreshScheduled();
  };

  return (
    <div
      className={clsx(
        "card flex items-center gap-3 p-3",
        !item.valid && "border-red-500/30",
      )}
    >
      <Clock size={16} className={item.enabled && item.valid ? "text-indigo-400" : "text-neutral-600"} />
      <div className="min-w-0 flex-1">
        <div className="flex items-center gap-2">
          <span className="truncate font-medium text-neutral-100">{item.title}</span>
          {!item.valid && (
            <span className="chip border-red-500/40 bg-red-500/10 text-red-300">
              <AlertTriangle size={11} /> invalid
            </span>
          )}
        </div>
        <div className="mt-0.5 flex items-center gap-2 text-xs text-neutral-500">
          {project && (
            <Link to={`/projects/${project.id}`} className="text-indigo-300 hover:underline">
              {project.name}
            </Link>
          )}
          <span className="flex items-center gap-1">
            <FileText size={11} /> {item.relPath}
          </span>
        </div>
        {item.error && <div className="mt-1 text-xs text-red-400">{item.error}</div>}
      </div>
      <div className="hidden text-xs text-neutral-400 sm:block">{item.scheduleDesc}</div>
      {item.agent && <AgentBadge agent={item.agent} />}
      {item.model && <span className="text-xs text-neutral-500">{item.model}</span>}
      <div className="w-20 text-right text-xs text-neutral-400" title={item.nextRun ?? ""}>
        {item.valid ? nextRunLabel(item.nextRun) : "—"}
      </div>
      <Switch
        checked={item.enabled}
        onChange={toggle}
        label={item.enabled ? "Disable scheduled task" : "Enable scheduled task"}
      />
    </div>
  );
}

export function ScheduledView() {
  const scheduled = useStore((s) => s.scheduled);
  const refreshScheduled = useStore((s) => s.refreshScheduled);

  useEffect(() => {
    refreshScheduled();
  }, [refreshScheduled]);

  const rescan = async () => {
    await api.refreshScheduled();
    await refreshScheduled();
  };

  return (
    <div className="p-6">
      <div className="mb-5 flex items-center justify-between">
        <div>
          <h1 className="text-lg font-semibold text-neutral-100">Scheduled tasks</h1>
          <p className="text-xs text-neutral-500">
            Discovered from <code className="rounded bg-[var(--color-surface-2)] px-1">.orchestrator/scheduled/*.md</code> across all projects.
          </p>
        </div>
        <button className="btn" onClick={rescan}>
          <RefreshCw size={14} /> Rescan
        </button>
      </div>

      {scheduled.length === 0 ? (
        <EmptyState
          icon={<Clock size={40} />}
          title="No scheduled tasks"
          hint={
            "Add a markdown file under .orchestrator/scheduled/ in a project with front matter like " +
            "schedule: \"0 9 * * *\" (cron) or every: 6h (interval). The body is the task prompt."
          }
        />
      ) : (
        <div className="flex flex-col gap-2">
          {scheduled.map((s) => (
            <Row key={s.id} item={s} />
          ))}
        </div>
      )}

      <div className="card mt-6 p-4 text-xs text-neutral-400">
        <div className="mb-2 font-semibold text-neutral-200">How scheduled tasks work</div>
        <p className="mb-2">
          Create a markdown file in a project's <code className="rounded bg-[var(--color-surface-2)] px-1">.orchestrator/scheduled/</code> folder. The front matter defines the schedule; the body is the prompt handed to the agent when it fires.
        </p>
        <pre className="overflow-x-auto rounded bg-black/30 p-3 text-[11px] text-neutral-300">{`---
schedule: "0 9 * * *"   # cron (5 or 6 fields), or:  every: 6h
agent: claude            # optional; omit to load-balance
model: opus              # optional; default = latest
priority: high
enabled: true
title: Daily dependency check
---
Check for outdated dependencies and open follow-up tasks to update them.`}</pre>
      </div>
    </div>
  );
}
