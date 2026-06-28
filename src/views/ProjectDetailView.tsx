import { useEffect, useMemo, useState } from "react";
import { useParams, useNavigate, Link } from "react-router-dom";
import { openPath } from "@tauri-apps/plugin-opener";
import {
  ArrowLeft, Plus, Sparkles, FolderOpen, FileCog, Trash2, Save,
} from "lucide-react";
import { useStore } from "../store";
import * as api from "../api";
import type { Project, Session } from "../api/types";
import { TaskTable } from "../components/TaskTable";
import { CreateTaskModal } from "../components/CreateTaskModal";
import { SessionKindBadge, SessionStatusBadge, AgentBadge } from "../components/Badges";
import { formatCost, formatRelative } from "../lib/format";

function SessionsList({ projectId }: { projectId: string }) {
  const [sessions, setSessions] = useState<Session[]>([]);
  const tasks = useStore((s) => s.tasks);

  useEffect(() => {
    let active = true;
    const load = () => api.listSessions({ projectId }).then((s) => active && setSessions(s));
    load();
    const unlisten = api.onOrchestratorEvent((e) => {
      if (e.type === "sessionUpdated" || e.type === "statusChanged") load();
    });
    return () => {
      active = false;
      unlisten.then((u) => u());
    };
  }, [projectId]);

  if (sessions.length === 0) {
    return <div className="px-1 py-4 text-sm text-neutral-500">No sessions yet.</div>;
  }
  return (
    <div className="flex flex-col gap-1.5">
      {sessions.slice(0, 25).map((s) => {
        const title = s.taskId ? tasks.find((t) => t.id === s.taskId)?.title : null;
        return (
          <Link
            key={s.id}
            to={`/sessions/${s.id}`}
            className="flex items-center gap-3 rounded-md border border-[var(--color-border)] bg-[var(--color-surface)] px-3 py-2 text-sm hover:border-indigo-500/40"
          >
            <SessionKindBadge kind={s.kind} />
            <SessionStatusBadge status={s.status} />
            <AgentBadge agent={s.agent} />
            <span className="min-w-0 flex-1 truncate text-neutral-300">
              {title ?? (s.kind === "roadmap" ? "Roadmap planning" : s.prompt.slice(0, 80))}
            </span>
            <span className="text-xs text-neutral-500">{formatCost(s.usage.totalCostUsd)}</span>
            <span className="text-xs text-neutral-600">{formatRelative(s.createdAt)}</span>
          </Link>
        );
      })}
    </div>
  );
}

function ProjectSettings({ project }: { project: Project }) {
  const refreshProjects = useStore((s) => s.refreshProjects);
  const [draft, setDraft] = useState(project);
  const [saved, setSaved] = useState(false);
  useEffect(() => setDraft(project), [project]);

  const dirty = JSON.stringify(draft) !== JSON.stringify(project);
  const save = async () => {
    await api.updateProject(draft);
    await refreshProjects();
    setSaved(true);
    setTimeout(() => setSaved(false), 1500);
  };

  const AGENTS: Project["defaultAgent"][] = ["claude", "gemini", "codex"];
  const toggleAgent = (a: Project["defaultAgent"]) => {
    const has = draft.allowedAgents.includes(a);
    let next = has
      ? draft.allowedAgents.filter((x) => x !== a)
      : [...draft.allowedAgents, a];
    if (next.length === 0) next = [a]; // never empty
    // Keep the default agent inside the allowed set.
    const defaultAgent = next.includes(draft.defaultAgent) ? draft.defaultAgent : next[0];
    setDraft({ ...draft, allowedAgents: next, defaultAgent });
  };

  return (
    <div className="card p-4">
      <h3 className="mb-3 text-sm font-semibold text-neutral-200">Project settings</h3>
      <div className="mb-4">
        <div className="mb-1 text-xs text-neutral-400">Allowed agents</div>
        <div className="flex gap-2">
          {AGENTS.map((a) => {
            const on = draft.allowedAgents.includes(a);
            return (
              <button
                key={a}
                type="button"
                onClick={() => toggleAgent(a)}
                className={`chip border ${
                  on
                    ? "border-indigo-500/50 bg-indigo-600/15 text-indigo-200"
                    : "border-[var(--color-border)] text-neutral-500 hover:text-neutral-300"
                }`}
              >
                {a}
              </button>
            );
          })}
        </div>
        <p className="mt-1 text-[11px] text-neutral-500">
          When more than one is enabled, unpinned tasks are load-balanced to the least-used agent.
        </p>
      </div>
      <div className="grid grid-cols-2 gap-4">
        <label className="flex items-center justify-between text-sm text-neutral-300">
          Enabled
          <input type="checkbox" checked={draft.enabled} onChange={(e) => setDraft({ ...draft, enabled: e.target.checked })} />
        </label>
        <label className="flex items-center justify-between text-sm text-neutral-300">
          Roadmap loop
          <input type="checkbox" checked={draft.roadmapEnabled} onChange={(e) => setDraft({ ...draft, roadmapEnabled: e.target.checked })} />
        </label>
        <label className="flex items-center justify-between text-sm text-neutral-300">
          Auto-verify
          <input type="checkbox" checked={draft.verifyEnabled} onChange={(e) => setDraft({ ...draft, verifyEnabled: e.target.checked })} />
        </label>
        <label className="flex items-center justify-between gap-2 text-sm text-neutral-300">
          Default agent
          <select
            className="input max-w-[120px]"
            value={draft.defaultAgent}
            onChange={(e) => setDraft({ ...draft, defaultAgent: e.target.value as Project["defaultAgent"] })}
          >
            {draft.allowedAgents.map((a) => (
              <option key={a} value={a}>{a}</option>
            ))}
          </select>
        </label>
        <label className="flex items-center justify-between gap-2 text-sm text-neutral-300">
          Max concurrent
          <input
            type="number"
            min={0}
            className="input max-w-[100px]"
            value={draft.maxConcurrent ?? ""}
            placeholder="global"
            onChange={(e) =>
              setDraft({ ...draft, maxConcurrent: e.target.value === "" ? null : Number(e.target.value) })
            }
          />
        </label>
      </div>
      <div className="mt-4 flex items-center gap-2">
        <button className="btn btn-primary" onClick={save} disabled={!dirty}>
          <Save size={14} /> Save
        </button>
        {saved && <span className="text-xs text-emerald-400">Saved</span>}
      </div>
    </div>
  );
}

export function ProjectDetailView() {
  const { id = "" } = useParams();
  const navigate = useNavigate();
  const projects = useStore((s) => s.projects);
  const tasks = useStore((s) => s.tasks);
  const refreshProjects = useStore((s) => s.refreshProjects);
  const [creating, setCreating] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);

  const project = projects.find((p) => p.id === id);
  const projectTasks = useMemo(() => tasks.filter((t) => t.projectId === id), [tasks, id]);

  useEffect(() => {
    if (projects.length === 0) refreshProjects();
  }, [projects.length, refreshProjects]);

  if (!project) {
    return <div className="p-6 text-sm text-neutral-500">Project not found.</div>;
  }

  const triggerRoadmap = async () => {
    await api.triggerRoadmap(project.id);
    setNotice("Roadmap planning queued.");
    setTimeout(() => setNotice(null), 2500);
  };
  const scaffold = async () => {
    const created = await api.scaffoldProject(project.id);
    setNotice(created.length ? `Created ${created.join(", ")}` : "Convention files already present.");
    setTimeout(() => setNotice(null), 3500);
  };
  const remove = async () => {
    await api.removeProject(project.id);
    await refreshProjects();
    navigate("/projects");
  };

  return (
    <div className="p-6">
      <button className="mb-3 flex items-center gap-1 text-xs text-neutral-400 hover:text-neutral-200" onClick={() => navigate("/projects")}>
        <ArrowLeft size={14} /> Projects
      </button>

      <div className="mb-5 flex items-start justify-between">
        <div>
          <h1 className="text-lg font-semibold text-neutral-100">{project.name}</h1>
          <div className="mt-0.5 truncate text-xs text-neutral-500" title={project.path}>{project.path}</div>
        </div>
        <div className="flex flex-wrap gap-2">
          <button className="btn" onClick={triggerRoadmap}><Sparkles size={14} /> Run roadmap</button>
          <button className="btn" onClick={scaffold}><FileCog size={14} /> Scaffold</button>
          <button className="btn" onClick={() => openPath(project.path)}><FolderOpen size={14} /> Open</button>
          <button className="btn btn-primary" onClick={() => setCreating(true)}><Plus size={14} /> Task</button>
          <button className="btn btn-danger" onClick={remove}><Trash2 size={14} /></button>
        </div>
      </div>

      {notice && <div className="mb-4 rounded-md border border-indigo-500/30 bg-indigo-600/10 px-3 py-2 text-xs text-indigo-200">{notice}</div>}

      <div className="mb-5"><ProjectSettings project={project} /></div>

      <h3 className="mb-2 text-sm font-semibold text-neutral-200">Tasks ({projectTasks.length})</h3>
      <div className="mb-6"><TaskTable tasks={projectTasks} /></div>

      <h3 className="mb-2 text-sm font-semibold text-neutral-200">Recent sessions</h3>
      <SessionsList projectId={project.id} />

      {creating && <CreateTaskModal projectId={project.id} lockProject onClose={() => setCreating(false)} />}
    </div>
  );
}
