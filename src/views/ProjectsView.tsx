import { useEffect, useMemo, useState } from "react";
import { useNavigate } from "react-router-dom";
import { open } from "@tauri-apps/plugin-dialog";
import { FolderGit2, Plus, FolderOpen, Power } from "lucide-react";
import { useStore } from "../store";
import * as api from "../api";
import type { Project } from "../api/types";
import { AgentBadge } from "../components/Badges";
import { Modal, EmptyState } from "../components/Modal";

function AddProjectModal({ onClose }: { onClose: () => void }) {
  const refreshProjects = useStore((s) => s.refreshProjects);
  const [path, setPath] = useState("");
  const [name, setName] = useState("");
  const [scaffold, setScaffold] = useState(true);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const pick = async () => {
    const selected = await open({ directory: true, multiple: false, title: "Select a git repository" });
    if (typeof selected === "string") {
      setPath(selected);
      if (!name) setName(selected.split(/[/\\]/).pop() ?? "");
    }
  };

  const submit = async () => {
    if (!path) {
      setError("Choose a project folder.");
      return;
    }
    setBusy(true);
    setError(null);
    try {
      await api.addProject({ path, name: name || null, scaffold });
      await refreshProjects();
      onClose();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Modal title="Add project" onClose={onClose}>
      <div className="flex flex-col gap-3">
        <div>
          <label className="mb-1 block text-xs text-neutral-400">Repository folder</label>
          <div className="flex gap-2">
            <input
              className="input"
              placeholder="/path/to/repo"
              value={path}
              onChange={(e) => setPath(e.target.value)}
            />
            <button className="btn shrink-0" onClick={pick}>
              <FolderOpen size={15} /> Browse
            </button>
          </div>
        </div>
        <div>
          <label className="mb-1 block text-xs text-neutral-400">Name</label>
          <input className="input" value={name} onChange={(e) => setName(e.target.value)} placeholder="my-project" />
        </div>
        <label className="flex items-center gap-2 text-sm text-neutral-300">
          <input type="checkbox" checked={scaffold} onChange={(e) => setScaffold(e.target.checked)} />
          Scaffold <code className="rounded bg-[var(--color-surface-2)] px-1 text-xs">.orchestrator/</code> convention files
        </label>
        {error && <div className="text-xs text-red-400">{error}</div>}
        <div className="mt-1 flex justify-end gap-2">
          <button className="btn" onClick={onClose}>Cancel</button>
          <button className="btn btn-primary" onClick={submit} disabled={busy}>
            {busy ? "Adding…" : "Add project"}
          </button>
        </div>
      </div>
    </Modal>
  );
}

function ProjectCard({ project }: { project: Project }) {
  const navigate = useNavigate();
  const tasks = useStore((s) => s.tasks);
  const refreshProjects = useStore((s) => s.refreshProjects);

  const counts = useMemo(() => {
    const mine = tasks.filter((t) => t.projectId === project.id);
    return {
      pending: mine.filter((t) => t.status === "pending" || t.status === "needs_review").length,
      running: mine.filter((t) => t.status === "running").length,
      done: mine.filter((t) => t.status === "completed").length,
      total: mine.length,
    };
  }, [tasks, project.id]);

  const toggleEnabled = async (e: React.MouseEvent) => {
    e.stopPropagation();
    await api.updateProject({ ...project, enabled: !project.enabled });
    await refreshProjects();
  };

  return (
    <div
      className="card cursor-pointer p-4 transition-colors hover:border-indigo-500/50"
      onClick={() => navigate(`/projects/${project.id}`)}
    >
      <div className="flex items-start justify-between">
        <div className="flex items-center gap-2">
          <FolderGit2 size={18} className="text-indigo-400" />
          <span className="font-medium text-neutral-100">{project.name}</span>
        </div>
        <button
          className={`btn !px-2 !py-1 ${project.enabled ? "text-emerald-300" : "text-neutral-500"}`}
          title={project.enabled ? "Enabled — click to disable" : "Disabled — click to enable"}
          onClick={toggleEnabled}
        >
          <Power size={14} />
        </button>
      </div>
      <div className="mt-1 truncate text-xs text-neutral-500" title={project.path}>
        {project.path}
      </div>
      <div className="mt-3 flex items-center gap-3 text-xs text-neutral-400">
        <AgentBadge agent={project.defaultAgent} />
        <span className="text-amber-300">{counts.pending} pending</span>
        <span className="text-indigo-300">{counts.running} running</span>
        <span className="text-emerald-300">{counts.done} done</span>
      </div>
      <div className="mt-2 flex gap-2 text-[11px] text-neutral-600">
        {project.roadmapEnabled && <span>roadmap loop</span>}
        {project.verifyEnabled && <span>· verify</span>}
      </div>
    </div>
  );
}

export function ProjectsView() {
  const projects = useStore((s) => s.projects);
  const refreshAll = useStore((s) => s.refreshAll);
  const [adding, setAdding] = useState(false);

  useEffect(() => {
    refreshAll();
  }, [refreshAll]);

  return (
    <div className="p-6">
      <div className="mb-5 flex items-center justify-between">
        <div>
          <h1 className="text-lg font-semibold text-neutral-100">Projects</h1>
          <p className="text-xs text-neutral-500">Local git repositories the orchestrator manages.</p>
        </div>
        <button className="btn btn-primary" onClick={() => setAdding(true)}>
          <Plus size={15} /> Add project
        </button>
      </div>

      {projects.length === 0 ? (
        <EmptyState
          icon={<FolderGit2 size={40} />}
          title="No projects yet"
          hint="Add a local git repository to start orchestrating autonomous agents against it."
          action={
            <button className="btn btn-primary" onClick={() => setAdding(true)}>
              <Plus size={15} /> Add your first project
            </button>
          }
        />
      ) : (
        <div className="grid grid-cols-1 gap-3 sm:grid-cols-2 xl:grid-cols-3">
          {projects.map((p) => (
            <ProjectCard key={p.id} project={p} />
          ))}
        </div>
      )}

      {adding && <AddProjectModal onClose={() => setAdding(false)} />}
    </div>
  );
}
