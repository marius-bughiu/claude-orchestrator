import { useEffect, useState } from "react";
import { useNavigate } from "react-router-dom";
import { AlertTriangle, Clock, RefreshCcw, GitFork, Unlink } from "lucide-react";
import * as api from "../api";
import type { StuckTask } from "../api/types";

const REASON: Record<string, { icon: typeof Clock; label: string }> = {
  running_long: { icon: Clock, label: "running long" },
  many_retries: { icon: RefreshCcw, label: "repeated retries" },
  dependency_cycle: { icon: GitFork, label: "dependency cycle" },
  missing_dependency: { icon: Unlink, label: "missing prerequisite" },
};

/// Surfaces tasks that may need a human: long-running sessions or tasks the
/// verifier keeps bouncing toward their attempt limit. Hidden when all clear.
export function NeedsAttention() {
  const [stuck, setStuck] = useState<StuckTask[]>([]);
  const navigate = useNavigate();

  useEffect(() => {
    let active = true;
    const load = () => api.stuckTasks().then((s) => active && setStuck(s)).catch(() => {});
    load();
    const unlisten = api.onOrchestratorEvent((e) => {
      if (e.type === "taskUpdated" || e.type === "sessionUpdated" || e.type === "statusChanged") load();
    });
    return () => { active = false; unlisten.then((u) => u()); };
  }, []);

  if (stuck.length === 0) return null;

  return (
    <div className="card mb-5 border-amber-500/30 bg-amber-500/5 p-4">
      <h3 className="mb-2 flex items-center gap-2 text-sm font-semibold text-amber-300">
        <AlertTriangle size={15} /> Needs attention ({stuck.length})
      </h3>
      <div className="flex flex-col gap-1.5">
        {stuck.map((s) => {
          const r = REASON[s.reason] ?? { icon: AlertTriangle, label: s.reason };
          const Icon = r.icon;
          return (
            <button
              key={s.task.id}
              onClick={() => navigate(`/tasks/${s.task.id}`)}
              className="flex items-center gap-3 rounded-md border border-[var(--color-border)] bg-[var(--color-surface)] px-3 py-2 text-left text-sm hover:border-amber-500/40"
            >
              <Icon size={14} className="shrink-0 text-amber-400" />
              <span className="min-w-0 flex-1 truncate text-neutral-200">{s.task.title}</span>
              <span className="shrink-0 text-[11px] text-amber-400/90">{r.label}</span>
              <span className="shrink-0 text-[11px] text-neutral-500">{s.detail}</span>
            </button>
          );
        })}
      </div>
    </div>
  );
}
