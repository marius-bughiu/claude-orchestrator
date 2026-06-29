import { useEffect, useMemo, useState } from "react";
import { useNavigate } from "react-router-dom";
import * as api from "../api";
import { useStore } from "../store";
import type { Task, TaskStatus } from "../api/types";
import { AgentBadge, PriorityBadge } from "../components/Badges";
import { EmptyState } from "../components/Modal";

/// The board columns, in flow order. Several live statuses collapse into the
/// "In progress" column so the board stays readable.
const COLUMNS: { key: string; label: string; statuses: TaskStatus[]; accent: string }[] = [
  { key: "pending", label: "To do", statuses: ["pending", "blocked"], accent: "border-t-neutral-500" },
  { key: "active", label: "In progress", statuses: ["queued", "running"], accent: "border-t-sky-500" },
  { key: "needs_review", label: "Needs review", statuses: ["needs_review"], accent: "border-t-amber-500" },
  { key: "completed", label: "Done", statuses: ["completed"], accent: "border-t-emerald-500" },
  { key: "failed", label: "Failed", statuses: ["failed", "cancelled"], accent: "border-t-rose-500" },
];

/// Compact "time until" label for a future ISO timestamp.
function untilLabel(iso: string): string {
  const s = Math.max(0, Math.round((new Date(iso).getTime() - Date.now()) / 1000));
  if (s < 60) return `${s}s`;
  const m = Math.round(s / 60);
  if (m < 60) return `${m}m`;
  return `${Math.round(m / 60)}h`;
}

// When a card is dropped on a column, the task moves to this status.
const DROP_STATUS: Record<string, TaskStatus> = {
  pending: "pending",
  active: "pending", // re-queue; the scheduler promotes it to running
  needs_review: "needs_review",
  completed: "completed",
  failed: "failed",
};

export function BoardView() {
  const tasks = useStore((s) => s.tasks);
  const projects = useStore((s) => s.projects);
  const refreshAll = useStore((s) => s.refreshAll);
  const refreshTasks = useStore((s) => s.refreshTasks);
  const [projectFilter, setProjectFilter] = useState("all");
  const [dragId, setDragId] = useState<string | null>(null);
  const [overCol, setOverCol] = useState<string | null>(null);
  const [focusedId, setFocusedId] = useState<string | null>(null);
  const navigate = useNavigate();

  useEffect(() => {
    refreshAll();
  }, [refreshAll]);

  const visible = useMemo(
    () => tasks.filter((t) => projectFilter === "all" || t.projectId === projectFilter),
    [tasks, projectFilter],
  );

  // Columns with their member tasks — shared by render and keyboard nav.
  const columns = useMemo(
    () => COLUMNS.map((c) => ({ ...c, items: visible.filter((t) => c.statuses.includes(t.status)) })),
    [visible],
  );

  const projectName = (id: string) => projects.find((p) => p.id === id)?.name ?? "";

  async function moveTask(task: Task, colKey: string) {
    const next = DROP_STATUS[colKey];
    if (!next || task.status === next) return;
    const updated: Task = { ...task, status: next };
    // Moving back into the queue should let it run again from scratch.
    if (next === "pending") updated.attempts = 0;
    await api.updateTask(updated);
    await refreshTasks();
  }

  async function moveTo(colKey: string) {
    const task = tasks.find((t) => t.id === dragId);
    setDragId(null);
    setOverCol(null);
    if (task) await moveTask(task, colKey);
  }

  // Default keyboard focus to the first card; keep it valid as tasks change.
  useEffect(() => {
    if (visible.length === 0) { setFocusedId(null); return; }
    if (!focusedId || !visible.some((t) => t.id === focusedId)) {
      setFocusedId(visible[0].id);
    }
  }, [visible, focusedId]);

  // Keyboard triage: arrows navigate, digits 1–5 set status, Enter opens.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const el = document.activeElement;
      if (el && ["INPUT", "TEXTAREA", "SELECT"].includes(el.tagName)) return;
      if (!focusedId) return;
      // Locate the focused card within the columns grid.
      let ci = -1, ri = -1;
      columns.forEach((c, i) => {
        const r = c.items.findIndex((t) => t.id === focusedId);
        if (r >= 0) { ci = i; ri = r; }
      });
      if (ci < 0) return;
      const focusAt = (c: number, r: number) => {
        const col = columns[c];
        if (!col || col.items.length === 0) return;
        setFocusedId(col.items[Math.min(r, col.items.length - 1)].id);
      };
      const key = e.key;
      if (key === "ArrowDown" || key === "j") { e.preventDefault(); focusAt(ci, ri + 1); }
      else if (key === "ArrowUp" || key === "k") { e.preventDefault(); focusAt(ci, Math.max(0, ri - 1)); }
      else if (key === "ArrowRight" || key === "l") {
        e.preventDefault();
        for (let c = ci + 1; c < columns.length; c++) if (columns[c].items.length) { focusAt(c, ri); break; }
      } else if (key === "ArrowLeft" || key === "h") {
        e.preventDefault();
        for (let c = ci - 1; c >= 0; c--) if (columns[c].items.length) { focusAt(c, ri); break; }
      } else if (key === "Enter") {
        e.preventDefault();
        navigate(`/tasks/${focusedId}`);
      } else if (key >= "1" && key <= String(COLUMNS.length)) {
        e.preventDefault();
        const task = visible.find((t) => t.id === focusedId);
        if (task) moveTask(task, COLUMNS[Number(key) - 1].key);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [columns, focusedId, visible]);

  return (
    <div className="flex h-full flex-col p-6">
      <div className="mb-4 flex items-center justify-between">
        <div>
          <h1 className="text-lg font-semibold text-neutral-100">Board</h1>
          <p className="text-xs text-neutral-500">Drag cards, or use the keyboard: <kbd className="kbd">↑↓←→</kbd> navigate · <kbd className="kbd">1–5</kbd> set status · <kbd className="kbd">⏎</kbd> open.</p>
        </div>
        <select className="input max-w-[220px]" value={projectFilter} onChange={(e) => setProjectFilter(e.target.value)}>
          <option value="all">All projects</option>
          {projects.map((p) => (
            <option key={p.id} value={p.id}>{p.name}</option>
          ))}
        </select>
      </div>

      {projects.length === 0 ? (
        <EmptyState title="No projects" hint="Add a project to see its tasks on the board." />
      ) : (
        <div className="flex min-h-0 flex-1 gap-3 overflow-x-auto pb-2">
          {columns.map((col, colIdx) => {
            const items = col.items;
            return (
              <div
                key={col.key}
                className={`flex w-72 shrink-0 flex-col rounded-lg border border-t-2 ${col.accent} ${
                  overCol === col.key ? "border-indigo-500/50 bg-indigo-500/5" : "border-[var(--color-border)] bg-[var(--color-surface)]"
                }`}
                onDragOver={(e) => {
                  e.preventDefault();
                  setOverCol(col.key);
                }}
                onDragLeave={() => setOverCol((c) => (c === col.key ? null : c))}
                onDrop={() => moveTo(col.key)}
              >
                <div className="flex items-center justify-between border-b border-[var(--color-border)] px-3 py-2">
                  <span className="text-xs font-semibold uppercase tracking-wide text-neutral-300">
                    <span className="mr-1 text-neutral-600">{colIdx + 1}</span>{col.label}
                  </span>
                  <span className="chip border border-[var(--color-border)] text-neutral-400">{items.length}</span>
                </div>
                <div className="flex min-h-0 flex-1 flex-col gap-2 overflow-y-auto p-2">
                  {items.length === 0 && (
                    <p className="px-2 py-6 text-center text-xs text-neutral-600">No tasks</p>
                  )}
                  {items.map((t) => (
                    <div
                      key={t.id}
                      draggable
                      onDragStart={() => setDragId(t.id)}
                      onDragEnd={() => {
                        setDragId(null);
                        setOverCol(null);
                      }}
                      onMouseEnter={() => setFocusedId(t.id)}
                      onClick={() => navigate(`/tasks/${t.id}`)}
                      className={`cursor-pointer rounded-md border bg-[var(--color-bg)] p-2.5 text-sm transition ${
                        focusedId === t.id ? "border-indigo-500 ring-1 ring-indigo-500/40" : "border-[var(--color-border)] hover:border-neutral-600"
                      } ${dragId === t.id ? "opacity-50" : ""}`}
                    >
                      <div className="mb-1.5 line-clamp-2 font-medium text-neutral-200">{t.title}</div>
                      <div className="flex flex-wrap items-center gap-1.5">
                        <AgentBadge agent={t.agent} />
                        <PriorityBadge priority={t.priority} />
                        {t.retryAt && new Date(t.retryAt).getTime() > Date.now() && (
                          <span className="text-[11px] text-amber-400" title="Waiting out retry backoff">retry in {untilLabel(t.retryAt)}</span>
                        )}
                        {projectFilter === "all" && (
                          <span className="text-[11px] text-neutral-500">{projectName(t.projectId)}</span>
                        )}
                      </div>
                    </div>
                  ))}
                </div>
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}
