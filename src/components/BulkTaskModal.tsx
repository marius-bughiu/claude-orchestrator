import { useMemo, useState } from "react";
import * as api from "../api";
import { useStore } from "../store";
import type { AgentKind } from "../api/types";
import { Modal } from "./Modal";

/// Create many tasks at once by pasting a list. One task per line; markdown
/// checklist/bullet markers are stripped server-side.
export function BulkTaskModal({ projectId: fixedProject, onClose }: { projectId?: string; onClose: () => void }) {
  const projects = useStore((s) => s.projects);
  const refreshTasks = useStore((s) => s.refreshTasks);
  const [projectId, setProjectId] = useState(fixedProject ?? projects[0]?.id ?? "");
  const [text, setText] = useState("");
  const [priority, setPriority] = useState(50);
  const [agent, setAgent] = useState<AgentKind | "">("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // Live preview of how many tasks will be created.
  const count = useMemo(
    () =>
      text
        .split("\n")
        .map((l) => l.trim().replace(/^[-*]\s*/, "").replace(/^\d+\.\s*/, "").replace(/^\[[ xX]\]\s*/, "").trim())
        .filter((l) => l && !l.startsWith("#")).length,
    [text],
  );

  const project = projects.find((p) => p.id === projectId);
  const allowed = project?.allowedAgents ?? ["claude"];

  const submit = async () => {
    if (!projectId) { setError("Choose a project."); return; }
    if (count === 0) { setError("Paste at least one task line."); return; }
    setBusy(true);
    setError(null);
    try {
      const created = await api.createTasksBulk({
        projectId,
        text,
        priority,
        agent: agent || undefined,
      });
      await refreshTasks();
      if (created.length === 0) setError("No task lines found.");
      else onClose();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Modal title="Bulk add tasks" onClose={onClose}>
      <div className="flex flex-col gap-3">
        {!fixedProject && (
          <div>
            <label className="mb-1 block text-xs text-neutral-400">Project</label>
            <select className="input" value={projectId} onChange={(e) => setProjectId(e.target.value)}>
              {projects.map((p) => <option key={p.id} value={p.id}>{p.name}</option>)}
            </select>
          </div>
        )}
        <div>
          <label className="mb-1 block text-xs text-neutral-400">Tasks — one per line</label>
          <textarea
            className="input min-h-[180px] resize-y font-mono text-xs"
            placeholder={"- [ ] Add login page\n- Wire up the API\n1. Write tests"}
            value={text}
            onChange={(e) => setText(e.target.value)}
          />
        </div>
        <div className="flex gap-3">
          <label className="text-sm text-neutral-300">
            <span className="mb-1 block text-xs text-neutral-400">Priority</span>
            <input type="number" className="input !w-24" value={priority} onChange={(e) => setPriority(Number(e.target.value))} />
          </label>
          <label className="text-sm text-neutral-300">
            <span className="mb-1 block text-xs text-neutral-400">Agent</span>
            <select className="input" value={agent} onChange={(e) => setAgent(e.target.value as AgentKind | "")}>
              <option value="">Auto (balance)</option>
              {allowed.map((a) => <option key={a} value={a}>{a}</option>)}
            </select>
          </label>
        </div>
        {error && <div className="text-xs text-rose-400">{error}</div>}
        <div className="mt-1 flex items-center justify-between">
          <span className="text-xs text-neutral-500">{count} task{count === 1 ? "" : "s"} will be created</span>
          <div className="flex gap-2">
            <button className="btn" onClick={onClose}>Cancel</button>
            <button className="btn btn-primary" onClick={submit} disabled={busy || count === 0}>
              {busy ? "Creating…" : `Create ${count}`}
            </button>
          </div>
        </div>
      </div>
    </Modal>
  );
}
