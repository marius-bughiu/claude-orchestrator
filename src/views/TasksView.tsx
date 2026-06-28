import { useEffect, useMemo, useState } from "react";
import { Plus, Search } from "lucide-react";
import { useStore } from "../store";
import type { TaskStatus } from "../api/types";
import { TaskTable } from "../components/TaskTable";
import { CreateTaskModal } from "../components/CreateTaskModal";
import { UpcomingTasks } from "../components/UpcomingTasks";
import { EmptyState } from "../components/Modal";

const STATUS_FILTERS: { label: string; value: TaskStatus | "all" | "active" }[] = [
  { label: "All", value: "all" },
  { label: "Active", value: "active" },
  { label: "Pending", value: "pending" },
  { label: "Running", value: "running" },
  { label: "Needs review", value: "needs_review" },
  { label: "Completed", value: "completed" },
  { label: "Failed", value: "failed" },
];

export function TasksView() {
  const tasks = useStore((s) => s.tasks);
  const projects = useStore((s) => s.projects);
  const refreshAll = useStore((s) => s.refreshAll);
  const [projectFilter, setProjectFilter] = useState("all");
  const [statusFilter, setStatusFilter] = useState<string>("all");
  const [search, setSearch] = useState("");
  const [creating, setCreating] = useState(false);

  useEffect(() => {
    refreshAll();
  }, [refreshAll]);

  const filtered = useMemo(() => {
    const q = search.trim().toLowerCase();
    return tasks.filter((t) => {
      if (projectFilter !== "all" && t.projectId !== projectFilter) return false;
      if (q) {
        const hay = `${t.title} ${t.description} ${t.tags.join(" ")}`.toLowerCase();
        if (!hay.includes(q)) return false;
      }
      if (statusFilter === "all") return true;
      if (statusFilter === "active")
        return ["pending", "queued", "running", "needs_review"].includes(t.status);
      return t.status === statusFilter;
    });
  }, [tasks, projectFilter, statusFilter, search]);

  return (
    <div className="p-6">
      <div className="mb-5 flex items-center justify-between">
        <div>
          <h1 className="text-lg font-semibold text-neutral-100">Tasks</h1>
          <p className="text-xs text-neutral-500">All work across every project.</p>
        </div>
        <button className="btn btn-primary" onClick={() => setCreating(true)} disabled={projects.length === 0}>
          <Plus size={15} /> New task
        </button>
      </div>

      <div className="mb-3 flex flex-wrap items-center gap-2">
        <div className="relative max-w-[240px] flex-1">
          <Search size={14} className="pointer-events-none absolute left-2.5 top-1/2 -translate-y-1/2 text-neutral-500" />
          <input
            className="input pl-8"
            placeholder="Search tasks…"
            value={search}
            onChange={(e) => setSearch(e.target.value)}
          />
        </div>
        <select className="input max-w-[200px]" value={projectFilter} onChange={(e) => setProjectFilter(e.target.value)}>
          <option value="all">All projects</option>
          {projects.map((p) => (
            <option key={p.id} value={p.id}>{p.name}</option>
          ))}
        </select>
        <div className="flex flex-wrap gap-1">
          {STATUS_FILTERS.map((f) => (
            <button
              key={f.value}
              className={`chip border ${
                statusFilter === f.value
                  ? "border-indigo-500/50 bg-indigo-600/15 text-indigo-200"
                  : "border-[var(--color-border)] text-neutral-400 hover:text-neutral-200"
              }`}
              onClick={() => setStatusFilter(f.value)}
            >
              {f.label}
            </button>
          ))}
        </div>
      </div>

      {projects.length === 0 ? (
        <EmptyState title="No projects" hint="Add a project before creating tasks." />
      ) : (
        <>
          <UpcomingTasks
            projectId={projectFilter === "all" ? undefined : projectFilter}
            showProject={projectFilter === "all"}
          />
          <TaskTable tasks={filtered} showProject />
        </>
      )}

      {creating && <CreateTaskModal onClose={() => setCreating(false)} />}
    </div>
  );
}
