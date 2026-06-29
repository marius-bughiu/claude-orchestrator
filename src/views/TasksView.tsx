import { useEffect, useMemo, useState } from "react";
import { Plus, Search, ListPlus, Download } from "lucide-react";
import { useStore } from "../store";
import type { TaskStatus } from "../api/types";
import { TaskTable } from "../components/TaskTable";
import { CreateTaskModal } from "../components/CreateTaskModal";
import { BulkTaskModal } from "../components/BulkTaskModal";
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
  const [activeTags, setActiveTags] = useState<string[]>([]);
  const [creating, setCreating] = useState(false);
  const [bulkAdding, setBulkAdding] = useState(false);
  const newTaskNonce = useStore((s) => s.newTaskNonce);

  useEffect(() => {
    refreshAll();
  }, [refreshAll]);

  // Open the create modal when the command palette (or anything) requests it.
  useEffect(() => {
    if (newTaskNonce > 0 && projects.length > 0) setCreating(true);
  }, [newTaskNonce]); // eslint-disable-line react-hooks/exhaustive-deps

  // All tags present on tasks in the current project scope, sorted by frequency.
  const tagCounts = useMemo(() => {
    const counts = new Map<string, number>();
    for (const t of tasks) {
      if (projectFilter !== "all" && t.projectId !== projectFilter) continue;
      for (const tag of t.tags) counts.set(tag, (counts.get(tag) ?? 0) + 1);
    }
    return [...counts.entries()].sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0]));
  }, [tasks, projectFilter]);

  const toggleTag = (tag: string) =>
    setActiveTags((cur) => (cur.includes(tag) ? cur.filter((t) => t !== tag) : [...cur, tag]));

  const projectName = (id: string) => projects.find((p) => p.id === id)?.name ?? id;

  // Drop selected tags that no longer exist in scope (e.g. after a project switch)
  // so the user can never get stuck filtering on an unreachable, hidden chip.
  useEffect(() => {
    const present = new Set(tagCounts.map(([t]) => t));
    setActiveTags((cur) => (cur.every((t) => present.has(t)) ? cur : cur.filter((t) => present.has(t))));
  }, [tagCounts]);

  const filtered = useMemo(() => {
    const q = search.trim().toLowerCase();
    return tasks.filter((t) => {
      if (projectFilter !== "all" && t.projectId !== projectFilter) return false;
      // A task must carry every selected tag (AND) to narrow the list.
      if (activeTags.length && !activeTags.every((tag) => t.tags.includes(tag))) return false;
      if (q) {
        const hay = `${t.title} ${t.description} ${t.tags.join(" ")}`.toLowerCase();
        if (!hay.includes(q)) return false;
      }
      if (statusFilter === "all") return true;
      if (statusFilter === "active")
        return ["pending", "queued", "running", "needs_review"].includes(t.status);
      return t.status === statusFilter;
    });
  }, [tasks, projectFilter, statusFilter, search, activeTags]);

  // Export the currently-filtered task list as CSV.
  const exportCsv = () => {
    const cols = ["title", "project", "status", "priority", "agent", "attempts", "maxAttempts", "tags", "createdAt", "updatedAt"] as const;
    const esc = (v: string) => (/[",\n]/.test(v) ? `"${v.replace(/"/g, '""')}"` : v);
    const rows = filtered.map((t) =>
      [t.title, projectName(t.projectId), t.status, String(t.priority), t.agent, String(t.attempts), String(t.maxAttempts), t.tags.join(" "), t.createdAt, t.updatedAt]
        .map(esc)
        .join(","),
    );
    const blob = new Blob([[cols.join(","), ...rows].join("\n")], { type: "text/csv" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = "orchestrator-tasks.csv";
    a.click();
    URL.revokeObjectURL(url);
  };

  return (
    <div className="p-6">
      <div className="mb-5 flex items-center justify-between">
        <div>
          <h1 className="text-lg font-semibold text-neutral-100">Tasks</h1>
          <p className="text-xs text-neutral-500">All work across every project.</p>
        </div>
        <div className="flex gap-2">
          <button className="btn" onClick={exportCsv} disabled={filtered.length === 0} title="Export the filtered tasks as CSV">
            <Download size={15} /> CSV
          </button>
          <button className="btn" onClick={() => setBulkAdding(true)} disabled={projects.length === 0}>
            <ListPlus size={15} /> Bulk add
          </button>
          <button className="btn btn-primary" onClick={() => setCreating(true)} disabled={projects.length === 0}>
            <Plus size={15} /> New task
          </button>
        </div>
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

      {tagCounts.length > 0 && (
        <div className="mb-4 flex flex-wrap items-center gap-1.5">
          <span className="text-[11px] uppercase tracking-wide text-neutral-600">Tags</span>
          {tagCounts.map(([tag, count]) => (
            <button
              key={tag}
              className={`chip border ${
                activeTags.includes(tag)
                  ? "border-indigo-500/50 bg-indigo-600/15 text-indigo-200"
                  : "border-[var(--color-border)] text-neutral-400 hover:text-neutral-200"
              }`}
              onClick={() => toggleTag(tag)}
            >
              {tag} <span className="text-neutral-600">{count}</span>
            </button>
          ))}
          {activeTags.length > 0 && (
            <button className="ml-1 text-[11px] text-neutral-500 hover:text-neutral-300" onClick={() => setActiveTags([])}>
              clear
            </button>
          )}
        </div>
      )}

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
      {bulkAdding && <BulkTaskModal onClose={() => setBulkAdding(false)} />}
    </div>
  );
}
