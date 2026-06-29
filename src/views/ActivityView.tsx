import { useEffect, useMemo, useState } from "react";
import { useNavigate } from "react-router-dom";
import {
  CheckCircle2, XCircle, Clock, GitMerge, GitBranch, Github, Sparkles, Activity as ActivityIcon,
} from "lucide-react";
import * as api from "../api";
import { useStore } from "../store";
import type { ActivityEntry } from "../api/types";
import { formatRelative } from "../lib/format";
import { EmptyState } from "../components/Modal";

const KIND_ICON: Record<string, typeof ActivityIcon> = {
  scheduled: Clock,
  roadmap: Sparkles,
  github: Github,
  pr: GitMerge,
  branch: GitBranch,
};

function iconFor(e: ActivityEntry) {
  if (e.kind === "task") return e.level === "error" ? XCircle : CheckCircle2;
  return KIND_ICON[e.kind] ?? ActivityIcon;
}

function colorFor(level: string): string {
  if (level === "error") return "text-rose-400";
  if (level === "warn") return "text-amber-400";
  return "text-emerald-400";
}

export function ActivityView() {
  const projects = useStore((s) => s.projects);
  const refreshProjects = useStore((s) => s.refreshProjects);
  const [entries, setEntries] = useState<ActivityEntry[]>([]);
  const [projectFilter, setProjectFilter] = useState("all");
  const navigate = useNavigate();

  useEffect(() => {
    if (projects.length === 0) refreshProjects();
  }, [projects.length, refreshProjects]);

  useEffect(() => {
    let active = true;
    const load = () =>
      api.getActivity(300, projectFilter === "all" ? undefined : projectFilter)
        .then((e) => active && setEntries(e))
        .catch(() => {});
    load();
    const unlisten = api.onOrchestratorEvent((ev) => {
      if (ev.type === "activity") load();
    });
    return () => { active = false; unlisten.then((u) => u()); };
  }, [projectFilter]);

  const grouped = useMemo(() => {
    // Group by calendar day for readable history.
    const groups: { day: string; items: ActivityEntry[] }[] = [];
    for (const e of entries) {
      const day = new Date(e.createdAt).toLocaleDateString(undefined, { weekday: "short", month: "short", day: "numeric" });
      const last = groups[groups.length - 1];
      if (last && last.day === day) last.items.push(e);
      else groups.push({ day, items: [e] });
    }
    return groups;
  }, [entries]);

  const target = (e: ActivityEntry) =>
    e.sessionId ? `/sessions/${e.sessionId}` : e.taskId ? `/tasks/${e.taskId}` : e.projectId ? `/projects/${e.projectId}` : null;

  return (
    <div className="p-6">
      <div className="mb-5 flex items-center justify-between">
        <div>
          <h1 className="text-lg font-semibold text-neutral-100">Activity</h1>
          <p className="text-xs text-neutral-500">A persisted history of completions, failures, merges, and scheduled runs.</p>
        </div>
        <select className="input max-w-[220px]" value={projectFilter} onChange={(e) => setProjectFilter(e.target.value)}>
          <option value="all">All projects</option>
          {projects.map((p) => <option key={p.id} value={p.id}>{p.name}</option>)}
        </select>
      </div>

      {entries.length === 0 ? (
        <EmptyState icon={<ActivityIcon size={40} />} title="No activity yet" hint="Significant events will appear here as the orchestrator runs." />
      ) : (
        <div className="flex flex-col gap-5">
          {grouped.map((g) => (
            <div key={g.day}>
              <div className="mb-2 text-xs font-medium uppercase tracking-wide text-neutral-500">{g.day}</div>
              <div className="flex flex-col gap-1.5">
                {g.items.map((e) => {
                  const Icon = iconFor(e);
                  const to = target(e);
                  return (
                    <div
                      key={e.id}
                      className={`flex items-center gap-3 rounded-md border border-[var(--color-border)] bg-[var(--color-surface)] px-3 py-2 text-sm ${to ? "cursor-pointer hover:border-indigo-500/40" : ""}`}
                      onClick={() => to && navigate(to)}
                    >
                      <Icon size={15} className={`shrink-0 ${colorFor(e.level)}`} />
                      <span className="min-w-0 flex-1 truncate text-neutral-200">{e.message}</span>
                      {e.projectName && <span className="shrink-0 text-[11px] text-neutral-500">{e.projectName}</span>}
                      <span className="shrink-0 text-[11px] text-neutral-600">{formatRelative(e.createdAt)}</span>
                    </div>
                  );
                })}
              </div>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
