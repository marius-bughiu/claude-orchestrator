import { useEffect, useState } from "react";
import { useNavigate } from "react-router-dom";
import { CheckCircle2, Circle, Rocket, X } from "lucide-react";
import * as api from "../api";
import { useStore } from "../store";

const DISMISS_KEY = "orchestrator.gettingStartedDone";

interface Step {
  key: string;
  label: string;
  done: boolean;
  action?: { label: string; run: () => void };
}

/// A persistent onboarding checklist that tracks the first-run milestones and
/// disappears once they're all complete (or the user dismisses it).
export function GettingStarted() {
  const projects = useStore((s) => s.projects);
  const tasks = useStore((s) => s.tasks);
  const status = useStore((s) => s.status);
  const refreshStatus = useStore((s) => s.refreshStatus);
  const [agentReady, setAgentReady] = useState<boolean | null>(null);
  const [dismissed, setDismissed] = useState(() => localStorage.getItem(DISMISS_KEY) === "1");
  const navigate = useNavigate();

  useEffect(() => {
    api.agentHealth().then((hs) => setAgentReady(hs.some((h) => h.available))).catch(() => setAgentReady(false));
  }, []);

  const steps: Step[] = [
    { key: "agent", label: "Install an agent CLI (Claude, Gemini, or Codex)", done: agentReady === true },
    { key: "project", label: "Add a project", done: projects.length > 0, action: { label: "Add project", run: () => navigate("/projects") } },
    { key: "task", label: "Create a task (or let the roadmap generate one)", done: tasks.length > 0, action: { label: "Go to tasks", run: () => navigate("/tasks") } },
    { key: "run", label: "Start the orchestrator", done: !!status?.running, action: { label: "Start", run: async () => { await api.setRunning(true); await refreshStatus(); } } },
  ];

  const completed = steps.filter((s) => s.done).length;
  const allDone = completed === steps.length;

  // Hide once everything's done (and remember), or if dismissed.
  useEffect(() => {
    if (allDone) localStorage.setItem(DISMISS_KEY, "1");
  }, [allDone]);
  if (dismissed || allDone || agentReady === null) return null;

  const close = () => {
    localStorage.setItem(DISMISS_KEY, "1");
    setDismissed(true);
  };

  return (
    <div className="card mb-5 p-4">
      <div className="mb-3 flex items-center gap-2">
        <Rocket size={16} className="text-indigo-400" />
        <h3 className="text-sm font-semibold text-neutral-100">Getting started</h3>
        <span className="text-xs text-neutral-500">{completed} of {steps.length} done</span>
        <button onClick={close} className="ml-auto text-neutral-500 hover:text-neutral-300" title="Dismiss">
          <X size={15} />
        </button>
      </div>
      <div className="mb-3 h-1 overflow-hidden rounded-full bg-[var(--color-border)]">
        <div className="h-full bg-indigo-500 transition-all" style={{ width: `${(completed / steps.length) * 100}%` }} />
      </div>
      <div className="flex flex-col gap-1.5">
        {steps.map((s) => (
          <div key={s.key} className="flex items-center gap-2.5 text-sm">
            {s.done ? <CheckCircle2 size={16} className="shrink-0 text-emerald-400" /> : <Circle size={16} className="shrink-0 text-neutral-600" />}
            <span className={s.done ? "text-neutral-500 line-through" : "text-neutral-200"}>{s.label}</span>
            {!s.done && s.action && (
              <button className="btn !ml-auto !px-2 !py-0.5 text-xs" onClick={s.action.run}>{s.action.label}</button>
            )}
          </div>
        ))}
      </div>
    </div>
  );
}
