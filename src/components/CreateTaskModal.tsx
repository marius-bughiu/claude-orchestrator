import { useState } from "react";
import { useStore } from "../store";
import * as api from "../api";
import type { AgentKind } from "../api/types";
import { Modal } from "./Modal";

const AGENTS: AgentKind[] = ["claude", "gemini", "codex"];
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
  const refreshTasks = useStore((s) => s.refreshTasks);
  const [pid, setPid] = useState(projectId ?? projects[0]?.id ?? "");
  const [title, setTitle] = useState("");
  const [description, setDescription] = useState("");
  const [priority, setPriority] = useState(50);
  const [agent, setAgent] = useState<AgentKind | "">("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

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
      <div className="flex flex-col gap-3">
        {!lockProject && (
          <div>
            <label className="mb-1 block text-xs text-neutral-400">Project</label>
            <select className="input" value={pid} onChange={(e) => setPid(e.target.value)}>
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
        <div className="grid grid-cols-2 gap-3">
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
              <option value="">Project default</option>
              {AGENTS.map((a) => (
                <option key={a} value={a}>{a}</option>
              ))}
            </select>
          </div>
        </div>
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
