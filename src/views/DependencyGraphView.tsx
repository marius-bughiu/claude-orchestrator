import { useEffect, useMemo, useState } from "react";
import { useNavigate } from "react-router-dom";
import * as api from "../api";
import { useStore } from "../store";
import type { Task } from "../api/types";
import { EmptyState } from "../components/Modal";

const COL_W = 240;
const ROW_H = 76;
const NODE_W = 200;
const NODE_H = 52;
const PAD = 24;

const STATUS_DOT: Record<string, string> = {
  pending: "bg-neutral-400",
  queued: "bg-sky-400",
  running: "bg-sky-400",
  needs_review: "bg-amber-400",
  completed: "bg-emerald-400",
  failed: "bg-rose-400",
  cancelled: "bg-neutral-600",
  blocked: "bg-neutral-500",
};

interface Placed {
  task: Task;
  x: number;
  y: number;
  level: number;
}

/// Assign each task a level = longest dependency chain behind it, so edges flow
/// left→right. Cycles are broken defensively by a visit cap.
function computeLevels(tasks: Task[], ids: Set<string>): Map<string, number> {
  const level = new Map<string, number>();
  const byId = new Map(tasks.map((t) => [t.id, t]));
  const visiting = new Set<string>();

  const depth = (id: string, guard: number): number => {
    if (level.has(id)) return level.get(id)!;
    if (guard > tasks.length || visiting.has(id)) return 0;
    visiting.add(id);
    const t = byId.get(id);
    const deps = (t?.dependsOn ?? []).filter((d) => ids.has(d));
    const d = deps.length === 0 ? 0 : 1 + Math.max(...deps.map((dep) => depth(dep, guard + 1)));
    visiting.delete(id);
    level.set(id, d);
    return d;
  };

  for (const t of tasks) depth(t.id, 0);
  return level;
}

export function DependencyGraphView() {
  const tasks = useStore((s) => s.tasks);
  const projects = useStore((s) => s.projects);
  const refreshAll = useStore((s) => s.refreshAll);
  const refreshTasks = useStore((s) => s.refreshTasks);
  const [projectFilter, setProjectFilter] = useState<string>("");
  const [selected, setSelected] = useState<string | null>(null);
  const navigate = useNavigate();

  useEffect(() => {
    refreshAll();
  }, [refreshAll]);

  // Default to the first project that has tasks.
  useEffect(() => {
    if (!projectFilter && projects.length > 0) {
      const withTasks = projects.find((p) => tasks.some((t) => t.projectId === p.id));
      setProjectFilter((withTasks ?? projects[0]).id);
    }
  }, [projectFilter, projects, tasks]);

  const projectTasks = useMemo(
    () => tasks.filter((t) => t.projectId === projectFilter),
    [tasks, projectFilter],
  );

  const { placed, width, height, edges } = useMemo(() => {
    const ids = new Set(projectTasks.map((t) => t.id));
    const levels = computeLevels(projectTasks, ids);
    const byLevel = new Map<number, Task[]>();
    for (const t of projectTasks) {
      const l = levels.get(t.id) ?? 0;
      if (!byLevel.has(l)) byLevel.set(l, []);
      byLevel.get(l)!.push(t);
    }
    const placed: Placed[] = [];
    const pos = new Map<string, Placed>();
    let maxLevel = 0;
    let maxRows = 0;
    for (const [l, group] of [...byLevel.entries()].sort((a, b) => a[0] - b[0])) {
      maxLevel = Math.max(maxLevel, l);
      maxRows = Math.max(maxRows, group.length);
      group.forEach((task, i) => {
        const p: Placed = { task, level: l, x: PAD + l * COL_W, y: PAD + i * ROW_H };
        placed.push(p);
        pos.set(task.id, p);
      });
    }
    const edges: { from: Placed; to: Placed }[] = [];
    for (const t of projectTasks) {
      for (const dep of t.dependsOn ?? []) {
        const from = pos.get(dep);
        const to = pos.get(t.id);
        if (from && to) edges.push({ from, to });
      }
    }
    return {
      placed,
      edges,
      width: PAD * 2 + (maxLevel + 1) * COL_W,
      height: PAD * 2 + maxRows * ROW_H,
    };
  }, [projectTasks]);

  const selectedTask = projectTasks.find((t) => t.id === selected) ?? null;

  const toggleDep = async (task: Task, depId: string) => {
    const has = task.dependsOn.includes(depId);
    const dependsOn = has ? task.dependsOn.filter((d) => d !== depId) : [...task.dependsOn, depId];
    await api.updateTask({ ...task, dependsOn });
    await refreshTasks();
  };

  return (
    <div className="flex h-full flex-col p-6">
      <div className="mb-4 flex items-center justify-between">
        <div>
          <h1 className="text-lg font-semibold text-neutral-100">Dependency graph</h1>
          <p className="text-xs text-neutral-500">Tasks flow left → right by dependency. Click a task to edit what it waits on.</p>
        </div>
        <select className="input max-w-[220px]" value={projectFilter} onChange={(e) => { setProjectFilter(e.target.value); setSelected(null); }}>
          {projects.map((p) => (
            <option key={p.id} value={p.id}>{p.name}</option>
          ))}
        </select>
      </div>

      {projects.length === 0 ? (
        <EmptyState title="No projects" hint="Add a project to see its task graph." />
      ) : projectTasks.length === 0 ? (
        <EmptyState title="No tasks" hint="This project has no tasks yet." />
      ) : (
        <div className="flex min-h-0 flex-1 gap-4">
          <div className="min-h-0 flex-1 overflow-auto rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)]">
            <div className="relative" style={{ width, height: Math.max(height, 200) }}>
              <svg className="absolute inset-0 h-full w-full" style={{ pointerEvents: "none" }}>
                <defs>
                  <marker id="arrow" markerWidth="8" markerHeight="8" refX="7" refY="3" orient="auto">
                    <path d="M0,0 L7,3 L0,6 Z" fill="#6366f1" />
                  </marker>
                </defs>
                {edges.map((e, i) => {
                  const x1 = e.from.x + NODE_W;
                  const y1 = e.from.y + NODE_H / 2;
                  const x2 = e.to.x;
                  const y2 = e.to.y + NODE_H / 2;
                  const mx = (x1 + x2) / 2;
                  return (
                    <path
                      key={i}
                      d={`M${x1},${y1} C${mx},${y1} ${mx},${y2} ${x2},${y2}`}
                      stroke="#6366f1"
                      strokeWidth={1.5}
                      fill="none"
                      opacity={0.5}
                      markerEnd="url(#arrow)"
                    />
                  );
                })}
              </svg>
              {placed.map((p) => (
                <button
                  key={p.task.id}
                  onClick={() => setSelected(p.task.id)}
                  onDoubleClick={() => navigate(`/tasks/${p.task.id}`)}
                  className={`absolute rounded-md border px-2.5 py-1.5 text-left text-xs transition ${
                    selected === p.task.id
                      ? "border-indigo-500 bg-indigo-500/10"
                      : "border-[var(--color-border)] bg-[var(--color-bg)] hover:border-neutral-600"
                  }`}
                  style={{ left: p.x, top: p.y, width: NODE_W, height: NODE_H }}
                >
                  <div className="flex items-center gap-1.5">
                    <span className={`h-2 w-2 shrink-0 rounded-full ${STATUS_DOT[p.task.status] ?? "bg-neutral-500"}`} />
                    <span className="line-clamp-2 leading-tight text-neutral-200">{p.task.title}</span>
                  </div>
                </button>
              ))}
            </div>
          </div>

          {selectedTask && (
            <div className="card w-72 shrink-0 overflow-y-auto p-4">
              <div className="mb-2 text-sm font-semibold text-neutral-200">{selectedTask.title}</div>
              <button className="mb-3 text-xs text-indigo-300 hover:underline" onClick={() => navigate(`/tasks/${selectedTask.id}`)}>
                open task →
              </button>
              <div className="mb-1.5 text-xs font-medium uppercase tracking-wide text-neutral-500">Depends on</div>
              <div className="flex flex-col gap-1">
                {projectTasks.filter((t) => t.id !== selectedTask.id).map((t) => (
                  <label key={t.id} className="flex items-center gap-2 text-xs text-neutral-300">
                    <input
                      type="checkbox"
                      checked={selectedTask.dependsOn.includes(t.id)}
                      onChange={() => toggleDep(selectedTask, t.id)}
                    />
                    <span className="truncate">{t.title}</span>
                  </label>
                ))}
              </div>
            </div>
          )}
        </div>
      )}
    </div>
  );
}
