import { useEffect, useMemo, useState } from "react";
import { useParams, useNavigate, Link } from "react-router-dom";
import { ArrowLeft, Play, Trash2, Save, RotateCcw, Copy, GitBranch, GitPullRequest, X, Plus } from "lucide-react";
import * as api from "../api";
import { useStore } from "../store";
import type { Session, Task } from "../api/types";
import { AgentBadge, PriorityBadge, TaskStatusBadge, SessionStatusBadge, SessionKindBadge } from "../components/Badges";
import { SessionDiffPanel } from "../components/SessionDiffPanel";
import { formatCost, formatRelative } from "../lib/format";

const STATUS_DOT: Record<string, string> = {
  pending: "bg-neutral-400", queued: "bg-sky-400", running: "bg-sky-400",
  needs_review: "bg-amber-400", completed: "bg-emerald-400", failed: "bg-rose-400",
  cancelled: "bg-neutral-600", blocked: "bg-neutral-500",
};

/// Compact "time until" label for a future ISO timestamp.
function untilLabel(iso: string): string {
  const s = Math.max(0, Math.round((new Date(iso).getTime() - Date.now()) / 1000));
  if (s < 60) return `${s}s`;
  const m = Math.round(s / 60);
  if (m < 60) return `${m}m`;
  return `${Math.round(m / 60)}h`;
}

function DepRow({ task, onRemove, onOpen }: { task: Task; onRemove?: () => void; onOpen: () => void }) {
  return (
    <div className="flex items-center gap-2 rounded-md border border-[var(--color-border)] bg-[var(--color-surface)] px-2.5 py-1.5 text-sm">
      <span className={`h-2 w-2 shrink-0 rounded-full ${STATUS_DOT[task.status] ?? "bg-neutral-500"}`} />
      <button className="min-w-0 flex-1 truncate text-left text-neutral-300 hover:text-neutral-100" onClick={onOpen}>
        {task.title}
      </button>
      {task.status !== "completed" && <span className="text-[11px] text-amber-400/80">blocking</span>}
      {onRemove && (
        <button className="text-neutral-500 hover:text-rose-400" onClick={onRemove} title="Remove dependency">
          <X size={14} />
        </button>
      )}
    </div>
  );
}

export function TaskDetailView() {
  const { id = "" } = useParams();
  const navigate = useNavigate();
  const projects = useStore((s) => s.projects);
  const allTasks = useStore((s) => s.tasks);
  const refreshTasks = useStore((s) => s.refreshTasks);
  const [task, setTask] = useState<Task | null>(null);
  const [sessions, setSessions] = useState<Session[]>([]);
  const [desc, setDesc] = useState("");
  const [saved, setSaved] = useState(false);
  const [addingDep, setAddingDep] = useState("");

  useEffect(() => {
    let active = true;
    const loadTask = () => api.getTask(id).then((t) => { if (active) { setTask(t); setDesc(t.description); } }).catch(() => {});
    const loadSessions = () => api.listSessions({ taskId: id }).then((s) => active && setSessions(s)).catch(() => {});
    loadTask();
    loadSessions();
    refreshTasks(); // ensure the store has sibling tasks for dependency editing
    const unlisten = api.onOrchestratorEvent((e) => {
      if (e.type === "taskUpdated" && e.task.id === id) { setTask(e.task); }
      if (e.type === "sessionUpdated") loadSessions();
    });
    return () => { active = false; unlisten.then((u) => u()); };
  }, [id]);

  const project = projects.find((p) => p.id === task?.projectId);
  const projectTasks = useMemo(
    () => allTasks.filter((t) => t.projectId === task?.projectId && t.id !== id),
    [allTasks, task?.projectId, id],
  );
  const blockers = useMemo(
    () => (task?.dependsOn ?? []).map((d) => projectTasks.find((t) => t.id === d)).filter(Boolean) as Task[],
    [task?.dependsOn, projectTasks],
  );
  const dependents = useMemo(
    () => projectTasks.filter((t) => t.dependsOn.includes(id)),
    [projectTasks, id],
  );
  // Most recent session that ran on an isolated branch — for inline changes.
  const branchSession = useMemo(() => sessions.find((s) => s.branch), [sessions]);

  if (!task) return <div className="p-6 text-sm text-neutral-500">Loading task…</div>;

  const save = async () => {
    await api.updateTask({ ...task, description: desc });
    setSaved(true);
    setTimeout(() => setSaved(false), 1500);
  };
  const patch = async (p: Partial<Task>) => {
    const next = { ...task, ...p };
    setTask(next);
    await api.updateTask(next);
    await refreshTasks();
  };
  const addDep = async (depId: string) => {
    if (!depId || task.dependsOn.includes(depId)) return;
    await patch({ dependsOn: [...task.dependsOn, depId] });
    setAddingDep("");
  };
  const removeDep = (depId: string) => patch({ dependsOn: task.dependsOn.filter((d) => d !== depId) });
  const runNow = async () => { await api.runTaskNow(task.id); };
  const retry = async () => { await api.retryTask(task.id); };
  const clone = async () => { const t = await api.cloneTask(task.id); await refreshTasks(); navigate(`/tasks/${t.id}`); };
  const del = async () => { await api.deleteTask(task.id); await refreshTasks(); navigate("/tasks"); };

  return (
    <div className="p-6">
      <button className="mb-3 flex items-center gap-1 text-xs text-neutral-400 hover:text-neutral-200" onClick={() => navigate(-1)}>
        <ArrowLeft size={14} /> Back
      </button>

      <div className="mb-4 flex items-start justify-between gap-4">
        <div className="min-w-0">
          <h1 className="truncate text-lg font-semibold text-neutral-100">{task.title}</h1>
          {project && (
            <Link to={`/projects/${project.id}`} className="text-xs text-indigo-300 hover:underline">{project.name}</Link>
          )}
        </div>
        <div className="flex flex-wrap gap-2">
          <button className="btn" onClick={runNow}><Play size={14} /> Run now</button>
          <button className="btn" onClick={retry} title="Reset attempts and re-queue"><RotateCcw size={14} /> Retry</button>
          <button className="btn" onClick={clone} title="Duplicate as a new task"><Copy size={14} /> Clone</button>
          <button className="btn btn-danger" onClick={del}><Trash2 size={14} /></button>
        </div>
      </div>

      <div className="mb-4 flex flex-wrap items-center gap-2">
        <TaskStatusBadge status={task.status} />
        <PriorityBadge priority={task.priority} />
        <AgentBadge agent={task.agent} />
        {task.autoGenerated && <span className="chip border-blue-500/40 bg-blue-500/10 text-blue-300">auto-generated</span>}
        <span className="text-xs text-neutral-500">attempts {task.attempts}/{task.maxAttempts}</span>
        {task.retryAt && new Date(task.retryAt) > new Date() && (
          <span className="chip border-amber-500/30 text-amber-400" title={`Backing off until ${new Date(task.retryAt).toLocaleString()}`}>
            retry in {untilLabel(task.retryAt)}
          </span>
        )}
        {branchSession?.branch && (
          <span className="inline-flex items-center gap-1 rounded bg-[var(--color-surface)] px-1.5 py-0.5 font-mono text-[11px] text-neutral-400">
            <GitBranch size={11} /> {branchSession.branch}
          </span>
        )}
        {branchSession?.prUrl && (
          <a href={branchSession.prUrl} target="_blank" rel="noreferrer" className="inline-flex items-center gap-1 rounded bg-emerald-500/10 px-1.5 py-0.5 text-[11px] text-emerald-300 hover:underline">
            <GitPullRequest size={11} /> PR
          </a>
        )}
        <label className="ml-auto flex items-center gap-1.5 text-xs text-neutral-400">
          priority
          <input
            type="number"
            className="input !w-20 !py-1"
            value={task.priority}
            onChange={(e) => patch({ priority: Number(e.target.value) })}
          />
        </label>
      </div>

      <div className="card mb-4 p-4">
        <label className="mb-1 block text-xs text-neutral-400">Instructions / acceptance criteria</label>
        <textarea
          className="input min-h-[160px] resize-y font-mono text-xs"
          value={desc}
          onChange={(e) => setDesc(e.target.value)}
        />
        <div className="mt-2 flex items-center gap-2">
          <button className="btn btn-primary" onClick={save} disabled={desc === task.description}>
            <Save size={14} /> Save
          </button>
          {saved && <span className="text-xs text-emerald-400">Saved</span>}
        </div>
      </div>

      <div className="mb-4 grid gap-4 lg:grid-cols-2">
        <div className="card p-4">
          <h3 className="mb-2 text-sm font-semibold text-neutral-200">Depends on</h3>
          <div className="flex flex-col gap-1.5">
            {blockers.length === 0 && <p className="text-xs text-neutral-500">No dependencies — schedulable immediately.</p>}
            {blockers.map((b) => (
              <DepRow key={b.id} task={b} onOpen={() => navigate(`/tasks/${b.id}`)} onRemove={() => removeDep(b.id)} />
            ))}
          </div>
          <div className="mt-2 flex items-center gap-2">
            <select className="input !py-1 text-xs" value={addingDep} onChange={(e) => addDep(e.target.value)}>
              <option value="">+ Add dependency…</option>
              {projectTasks
                .filter((t) => !task.dependsOn.includes(t.id) && !t.dependsOn.includes(id))
                .map((t) => <option key={t.id} value={t.id}>{t.title}</option>)}
            </select>
            <Plus size={14} className="text-neutral-600" />
          </div>
        </div>

        <div className="card p-4">
          <h3 className="mb-2 text-sm font-semibold text-neutral-200">Blocks</h3>
          <div className="flex flex-col gap-1.5">
            {dependents.length === 0 && <p className="text-xs text-neutral-500">No other tasks depend on this one.</p>}
            {dependents.map((d) => (
              <DepRow key={d.id} task={d} onOpen={() => navigate(`/tasks/${d.id}`)} />
            ))}
          </div>
        </div>
      </div>

      {branchSession && (
        <div className="card mb-6 overflow-hidden p-0">
          <SessionDiffPanel sessionId={branchSession.id} hasBranch={!!branchSession.branch} />
        </div>
      )}

      <h3 className="mb-2 text-sm font-semibold text-neutral-200">Sessions ({sessions.length})</h3>
      <div className="flex flex-col gap-1.5">
        {sessions.length === 0 && <div className="text-sm text-neutral-500">No sessions have run for this task yet.</div>}
        {sessions.map((s) => (
          <Link
            key={s.id}
            to={`/sessions/${s.id}`}
            className="flex items-center gap-3 rounded-md border border-[var(--color-border)] bg-[var(--color-surface)] px-3 py-2 text-sm hover:border-indigo-500/40"
          >
            <SessionKindBadge kind={s.kind} />
            <SessionStatusBadge status={s.status} />
            <span className="min-w-0 flex-1 truncate text-neutral-400">{s.resultText?.slice(0, 80) ?? s.error ?? "—"}</span>
            <span className="text-xs text-neutral-500">{formatCost(s.usage.totalCostUsd)}</span>
            <span className="text-xs text-neutral-600">{formatRelative(s.createdAt)}</span>
          </Link>
        ))}
      </div>
    </div>
  );
}
