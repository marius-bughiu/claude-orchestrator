import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { Rocket, FolderOpen, CheckCircle2, XCircle, Loader2, ArrowRight, Sparkles } from "lucide-react";
import * as api from "../api";
import { useStore } from "../store";
import type { AgentHealth } from "../api/types";
import { AGENT_LABELS } from "../lib/format";

const DISMISS_KEY = "orchestrator.onboarded";

type Step = "welcome" | "agents" | "project" | "done";

/// First-run guided setup. Renders as a full-screen overlay when there are no
/// projects yet and the user hasn't dismissed it. Walks through agent detection
/// and adding the first project.
export function Onboarding() {
  const connected = useStore((s) => s.connected);
  const projects = useStore((s) => s.projects);
  const refreshProjects = useStore((s) => s.refreshProjects);
  const [dismissed, setDismissed] = useState(() => localStorage.getItem(DISMISS_KEY) === "1");
  const [step, setStep] = useState<Step>("welcome");
  const [health, setHealth] = useState<AgentHealth[] | null>(null);
  const [path, setPath] = useState("");
  const [name, setName] = useState("");
  const [scaffold, setScaffold] = useState(true);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const show = connected && projects.length === 0 && !dismissed;

  useEffect(() => {
    if (show && step === "agents" && !health) {
      api.agentHealth().then(setHealth).catch(() => setHealth([]));
    }
  }, [show, step, health]);

  if (!show) return null;

  const close = () => {
    localStorage.setItem(DISMISS_KEY, "1");
    setDismissed(true);
  };

  const pick = async () => {
    const selected = await open({ directory: true, multiple: false, title: "Select a git repository" });
    if (typeof selected === "string") {
      setPath(selected);
      if (!name) setName(selected.split(/[/\\]/).pop() ?? "");
    }
  };

  const addProject = async () => {
    if (!path) {
      setError("Choose a project folder first.");
      return;
    }
    setBusy(true);
    setError(null);
    try {
      await api.addProject({ path, name: name || null, scaffold });
      await refreshProjects();
      setStep("done");
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  const anyAgent = health?.some((h) => h.available);

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-sm">
      <div className="card w-full max-w-lg p-6">
        {step === "welcome" && (
          <div className="text-center">
            <div className="mx-auto mb-4 flex h-14 w-14 items-center justify-center rounded-full bg-indigo-600/15 text-indigo-400">
              <Rocket size={28} />
            </div>
            <h2 className="mb-1 text-xl font-semibold text-neutral-100">Welcome to Claude Orchestrator</h2>
            <p className="mb-6 text-sm text-neutral-400">
              Run autonomous coding agents across your local repositories. Let's get you set up in two quick steps.
            </p>
            <div className="flex justify-center gap-2">
              <button className="btn" onClick={close}>Skip</button>
              <button className="btn btn-primary" onClick={() => setStep("agents")}>
                Get started <ArrowRight size={15} />
              </button>
            </div>
          </div>
        )}

        {step === "agents" && (
          <div>
            <h2 className="mb-1 text-lg font-semibold text-neutral-100">Agent CLIs</h2>
            <p className="mb-4 text-sm text-neutral-400">
              The orchestrator drives these command-line agents. At least one must be installed.
            </p>
            <div className="mb-5 flex flex-col gap-2">
              {!health && (
                <div className="flex items-center gap-2 py-4 text-sm text-neutral-500">
                  <Loader2 size={15} className="animate-spin" /> Detecting installed agents…
                </div>
              )}
              {health?.map((h) => (
                <div key={h.agent} className="flex items-center gap-3 rounded-md border border-[var(--color-border)] bg-[var(--color-surface)] px-3 py-2">
                  {h.available ? (
                    <CheckCircle2 size={16} className="text-emerald-400" />
                  ) : (
                    <XCircle size={16} className="text-neutral-600" />
                  )}
                  <span className="text-sm font-medium text-neutral-200">{AGENT_LABELS[h.agent]}</span>
                  <span className="ml-auto truncate text-xs text-neutral-500">
                    {h.available ? (h.version ?? "installed") : `not found (${h.binary})`}
                  </span>
                </div>
              ))}
            </div>
            {health && !anyAgent && (
              <p className="mb-4 text-xs text-amber-400/80">
                No agents detected. You can still add a project now and install an agent later — the orchestrator
                will pick it up automatically.
              </p>
            )}
            <div className="flex justify-between">
              <button className="btn" onClick={close}>Skip setup</button>
              <button className="btn btn-primary" onClick={() => setStep("project")}>
                Next <ArrowRight size={15} />
              </button>
            </div>
          </div>
        )}

        {step === "project" && (
          <div>
            <h2 className="mb-1 text-lg font-semibold text-neutral-100">Add your first project</h2>
            <p className="mb-4 text-sm text-neutral-400">Point the orchestrator at a local git repository.</p>
            <div className="mb-3">
              <label className="mb-1 block text-xs text-neutral-400">Repository folder</label>
              <div className="flex gap-2">
                <input className="input" placeholder="/path/to/repo" value={path} onChange={(e) => setPath(e.target.value)} />
                <button className="btn shrink-0" onClick={pick}>
                  <FolderOpen size={15} /> Browse
                </button>
              </div>
            </div>
            <div className="mb-3">
              <label className="mb-1 block text-xs text-neutral-400">Name</label>
              <input className="input" value={name} onChange={(e) => setName(e.target.value)} placeholder="my-project" />
            </div>
            <label className="mb-4 flex items-center gap-2 text-sm text-neutral-300">
              <input type="checkbox" checked={scaffold} onChange={(e) => setScaffold(e.target.checked)} />
              Scaffold <code className="rounded bg-[var(--color-surface-2)] px-1 text-xs">.orchestrator/</code> convention files
            </label>
            {error && <div className="mb-3 text-xs text-rose-400">{error}</div>}
            <div className="flex justify-between">
              <button className="btn" onClick={() => setStep("agents")}>Back</button>
              <button className="btn btn-primary" onClick={addProject} disabled={busy}>
                {busy ? "Adding…" : "Add project"}
              </button>
            </div>
          </div>
        )}

        {step === "done" && (
          <div className="text-center">
            <div className="mx-auto mb-4 flex h-14 w-14 items-center justify-center rounded-full bg-emerald-600/15 text-emerald-400">
              <Sparkles size={28} />
            </div>
            <h2 className="mb-1 text-xl font-semibold text-neutral-100">You're all set</h2>
            <p className="mb-6 text-sm text-neutral-400">
              Your project is added. Create a task or let the roadmap loop generate work, then press play to start the engine.
            </p>
            <button className="btn btn-primary" onClick={close}>Start orchestrating</button>
          </div>
        )}
      </div>
    </div>
  );
}
