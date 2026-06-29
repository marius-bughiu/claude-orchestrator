import { useMemo, useState } from "react";
import { X } from "lucide-react";
import { useStore } from "../store";
import * as api from "../api";
import type { AgentKind } from "../api/types";
import { Modal } from "./Modal";
import { ModelInput } from "./ModelInput";

/// Split a free-form tag string ("docs, ci urgent") into a clean, de-duped list.
function parseTags(raw: string): string[] {
  const seen = new Set<string>();
  return raw
    .split(/[,\s]+/)
    .map((t) => t.trim())
    .filter((t) => t && !seen.has(t) && seen.add(t));
}

const PRIORITIES = [
  { label: "Low", value: 0 },
  { label: "Normal", value: 50 },
  { label: "High", value: 100 },
  { label: "Urgent", value: 200 },
];

export function CreateTaskModal({
  projectId,
  lockProject = false,
  onClose,
}: {
  projectId?: string;
  lockProject?: boolean;
  onClose: () => void;
}) {
  const projects = useStore((s) => s.projects);
  const allTasks = useStore((s) => s.tasks);
  const refreshTasks = useStore((s) => s.refreshTasks);
  const [pid, setPid] = useState(projectId ?? projects[0]?.id ?? "");
  const [title, setTitle] = useState("");
  const [description, setDescription] = useState("");
  const [priority, setPriority] = useState(50);
  const [agent, setAgent] = useState<AgentKind | "">("");
  const [model, setModel] = useState("");
  const [tags, setTags] = useState("");
  const [dependsOn, setDependsOn] = useState<string[]>([]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const project = useMemo(() => projects.find((p) => p.id === pid), [projects, pid]);
  const allowedAgents = project?.allowedAgents ?? ["claude"];
  // The agent whose suggestions to show: explicit choice, else project default.
  const modelAgent: AgentKind = (agent || project?.defaultAgent || "claude") as AgentKind;

  // Candidate prerequisites: other tasks in the same project.
  const siblingTasks = useMemo(() => allTasks.filter((t) => t.projectId === pid), [allTasks, pid]);
  const depTitle = (id: string) => siblingTasks.find((t) => t.id === id)?.title ?? id;

  const submit = async () => {
    if (!pid) return setError("Select a project.");
    if (!title.trim()) return setError("Title is required.");
    setBusy(true);
    setError(null);
    try {
      await api.createTask({
        projectId: pid,
        title: title.trim(),
        description,
        priority,
        agent: agent || null,
        model: model.trim() || null,
        tags: parseTags(tags),
        dependsOn,
      });
      await refreshTasks();
      onClose();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Modal title="New task" onClose={onClose} width="max-w-xl">
      <div
        className="flex flex-col gap-3"
        onKeyDown={(e) => {
          if ((e.metaKey || e.ctrlKey) && e.key === "Enter") submit();
        }}
      >
        {!lockProject && (
          <div>
            <label className="mb-1 block text-xs text-neutral-400">Project</label>
            <select className="input" value={pid} onChange={(e) => { setPid(e.target.value); setAgent(""); setDependsOn([]); }}>
              {projects.map((p) => (
                <option key={p.id} value={p.id}>{p.name}</option>
              ))}
            </select>
          </div>
        )}
        <div>
          <label className="mb-1 block text-xs text-neutral-400">Title</label>
          <input className="input" value={title} onChange={(e) => setTitle(e.target.value)} placeholder="Add user authentication" />
        </div>
        <div>
          <label className="mb-1 block text-xs text-neutral-400">
            Instructions / acceptance criteria
          </label>
          <textarea
            className="input min-h-[120px] resize-y font-mono text-xs"
            value={description}
            onChange={(e) => setDescription(e.target.value)}
            placeholder="Describe the goal precisely. The executing agent has no other context."
          />
        </div>
        <div className="grid grid-cols-3 gap-3">
          <div>
            <label className="mb-1 block text-xs text-neutral-400">Priority</label>
            <select className="input" value={priority} onChange={(e) => setPriority(Number(e.target.value))}>
              {PRIORITIES.map((p) => (
                <option key={p.value} value={p.value}>{p.label}</option>
              ))}
            </select>
          </div>
          <div>
            <label className="mb-1 block text-xs text-neutral-400">Agent</label>
            <select className="input" value={agent} onChange={(e) => setAgent(e.target.value as AgentKind | "")}>
              <option value="">Auto / balanced</option>
              {allowedAgents.map((a) => (
                <option key={a} value={a}>{a}</option>
              ))}
            </select>
          </div>
          <div>
            <label className="mb-1 block text-xs text-neutral-400">Model</label>
            <ModelInput agent={modelAgent} value={model} onChange={setModel} id="task-model" />
          </div>
        </div>
        <div>
          <label className="mb-1 block text-xs text-neutral-400">Tags</label>
          <input
            className="input"
            value={tags}
            onChange={(e) => setTags(e.target.value)}
            placeholder="comma or space separated, e.g. docs, ci"
          />
        </div>
        {siblingTasks.length > 0 && (
          <div>
            <label className="mb-1 block text-xs text-neutral-400">Depends on</label>
            {dependsOn.length > 0 && (
              <div className="mb-1.5 flex flex-wrap gap-1.5">
                {dependsOn.map((d) => (
                  <span key={d} className="chip inline-flex items-center gap-1 border-indigo-500/40 bg-indigo-600/15 text-indigo-200">
                    <span className="max-w-[180px] truncate">{depTitle(d)}</span>
                    <button
                      className="text-indigo-300/70 hover:text-rose-400"
                      onClick={() => setDependsOn(dependsOn.filter((x) => x !== d))}
                      title="Remove"
                    >
                      <X size={12} />
                    </button>
                  </span>
                ))}
              </div>
            )}
            <select
              className="input"
              value=""
              onChange={(e) => { if (e.target.value) setDependsOn([...dependsOn, e.target.value]); }}
            >
              <option value="">+ Add a prerequisite task…</option>
              {siblingTasks
                .filter((t) => !dependsOn.includes(t.id))
                .map((t) => <option key={t.id} value={t.id}>{t.title}</option>)}
            </select>
            <p className="mt-1 text-[11px] text-neutral-500">This task stays blocked until its prerequisites complete.</p>
          </div>
        )}
        {allowedAgents.length === 1 && (
          <p className="text-[11px] text-neutral-500">
            This project only allows <span className="text-neutral-300">{allowedAgents[0]}</span>.
            Enable more agents in the project settings to balance load.
          </p>
        )}
        {error && <div className="text-xs text-red-400">{error}</div>}
        <div className="mt-1 flex justify-end gap-2">
          <button className="btn" onClick={onClose}>Cancel</button>
          <button className="btn btn-primary" onClick={submit} disabled={busy}>
            {busy ? "Creating…" : "Create task"}
          </button>
        </div>
      </div>
    </Modal>
  );
}
