import { useEffect, useMemo, useState } from "react";
import { GitBranch, ListTodo, Activity, AlertTriangle } from "lucide-react";
import * as api from "../api";
import { useStore } from "../store";
import type { GitStatus, Project, Session } from "../api/types";

function Stat({ label, value, hint, tone }: { label: string; value: string; hint?: string; tone?: string }) {
  return (
    <div>
      <div className="text-[11px] uppercase tracking-wide text-neutral-500">{label}</div>
      <div className={`text-base font-semibold ${tone ?? "text-neutral-100"}`}>{value}</div>
      {hint && <div className="text-[11px] text-neutral-600">{hint}</div>}
    </div>
  );
}

/// At-a-glance project health: git state, task queue breakdown, and recent
/// session success rate.
export function ProjectHealth({ project }: { project: Project }) {
  const tasks = useStore((s) => s.tasks);
  const [git, setGit] = useState<GitStatus | null>(null);
  const [sessions, setSessions] = useState<Session[]>([]);

  useEffect(() => {
    let active = true;
    api.projectGitStatus(project.id).then((g) => active && setGit(g)).catch(() => {});
    const load = () => api.listSessions({ projectId: project.id }).then((s) => active && setSessions(s)).catch(() => {});
    load();
    const unlisten = api.onOrchestratorEvent((e) => {
      if (e.type === "sessionUpdated") load();
      if (e.type === "taskUpdated") api.projectGitStatus(project.id).then((g) => active && setGit(g)).catch(() => {});
    });
    return () => { active = false; unlisten.then((u) => u()); };
  }, [project.id]);

  const counts = useMemo(() => {
    const mine = tasks.filter((t) => t.projectId === project.id);
    const by = (s: string) => mine.filter((t) => t.status === s).length;
    return {
      total: mine.length,
      pending: by("pending") + by("needs_review"),
      running: by("running") + by("queued"),
      failed: by("failed"),
      done: by("completed"),
    };
  }, [tasks, project.id]);

  const sess = useMemo(() => {
    const terminal = sessions.filter((s) => s.kind === "task" && ["completed", "failed", "cancelled", "timed_out"].includes(s.status));
    const ok = terminal.filter((s) => s.status === "completed").length;
    const active = sessions.filter((s) => s.status === "running" || s.status === "pending").length;
    const rate = terminal.length ? Math.round((ok / terminal.length) * 100) : null;
    return { rate, ok, total: terminal.length, active };
  }, [sessions]);

  return (
    <div className="card mb-5 p-4">
      <h3 className="mb-3 text-sm font-semibold text-neutral-200">Health</h3>
      <div className="grid grid-cols-1 gap-4 sm:grid-cols-3">
        <div className="flex items-start gap-2">
          <GitBranch size={16} className="mt-0.5 text-indigo-400" />
          {git?.available ? (
            <Stat
              label="Git"
              value={git.branch ?? "—"}
              hint={
                [
                  git.dirty ? "uncommitted changes" : "clean",
                  git.ahead ? `↑${git.ahead}` : "",
                  git.behind ? `↓${git.behind}` : "",
                ].filter(Boolean).join(" · ")
              }
              tone={git.dirty ? "text-amber-300" : "text-neutral-100"}
            />
          ) : (
            <Stat label="Git" value="not a repo" />
          )}
        </div>

        <div className="flex items-start gap-2">
          <ListTodo size={16} className="mt-0.5 text-indigo-400" />
          <Stat
            label="Tasks"
            value={`${counts.pending} queued`}
            hint={`${counts.running} running · ${counts.done} done · ${counts.failed} failed`}
            tone={counts.failed > 0 ? "text-amber-300" : "text-neutral-100"}
          />
        </div>

        <div className="flex items-start gap-2">
          {sess.rate !== null && sess.rate < 60 ? (
            <AlertTriangle size={16} className="mt-0.5 text-amber-400" />
          ) : (
            <Activity size={16} className="mt-0.5 text-indigo-400" />
          )}
          <Stat
            label="Session success"
            value={sess.rate === null ? "—" : `${sess.rate}%`}
            hint={`${sess.ok}/${sess.total} ok · ${sess.active} active`}
            tone={sess.rate !== null && sess.rate < 60 ? "text-amber-300" : "text-neutral-100"}
          />
        </div>
      </div>
    </div>
  );
}
