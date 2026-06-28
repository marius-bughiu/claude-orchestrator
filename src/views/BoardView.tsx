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
  const navigate = useNavigate();

  useEffect(() => {
    refreshAll();
  }, [refreshAll]);

  const visible = useMemo(
    () => tasks.filter((t) => projectFilter === "all" || t.projectId === projectFilter),
    [tasks, projectFilter],
  );

  const projectName = (id: string) => projects.find((p) => p.id === id)?.name ?? "";

  async function moveTo(colKey: string) {
    const task = tasks.find((t) => t.id === dragId);
    setDragId(null);
    setOverCol(null);
    if (!task) return;
    const next = DROP_STATUS[colKey];
    if (!next || task.status === next) return;
    const updated: Task = { ...task, status: next };
    // Moving back into the queue should let it run again from scratch.
    if (next === "pending") updated.attempts = 0;
    await api.updateTask(updated);
    await refreshTasks();
  }

  return (
    <div className="flex h-full flex-col p-6">
      <div className="mb-4 flex items-center justify-between">
        <div>
          <h1 className="text-lg font-semibold text-neutral-100">Board</h1>
          <p className="text-xs text-neutral-500">Drag cards between columns to change status.</p>
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
          {COLUMNS.map((col) => {
            const items = visible.filter((t) => col.statuses.includes(t.status));
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
                  <span className="text-xs font-semibold uppercase tracking-wide text-neutral-300">{col.label}</span>
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
                      onClick={() => navigate(`/tasks/${t.id}`)}
                      className={`cursor-pointer rounded-md border border-[var(--color-border)] bg-[var(--color-bg)] p-2.5 text-sm transition hover:border-neutral-600 ${
                        dragId === t.id ? "opacity-50" : ""
                      }`}
                    >
                      <div className="mb-1.5 line-clamp-2 font-medium text-neutral-200">{t.title}</div>
                      <div className="flex flex-wrap items-center gap-1.5">
                        <AgentBadge agent={t.agent} />
                        <PriorityBadge priority={t.priority} />
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
