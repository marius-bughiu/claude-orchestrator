import { useEffect, useState } from "react";
import { useNavigate } from "react-router-dom";
import { ListOrdered } from "lucide-react";
import * as api from "../api";
import type { QueuedTask } from "../api/types";
import { AgentBadge, PriorityBadge } from "./Badges";

/// A preview of the next tasks the scheduler will run, ordered by effective
/// (aged) priority across all enabled projects. Hidden when the queue is empty.
export function NextUpQueue() {
  const [queue, setQueue] = useState<QueuedTask[]>([]);
  const navigate = useNavigate();

  useEffect(() => {
    let active = true;
    const load = () => api.upcomingQueue(8).then((q) => active && setQueue(q)).catch(() => {});
    load();
    const unlisten = api.onOrchestratorEvent((e) => {
      if (e.type === "taskUpdated" || e.type === "sessionUpdated" || e.type === "statusChanged") load();
    });
    return () => { active = false; unlisten.then((u) => u()); };
  }, []);

  if (queue.length === 0) return null;

  return (
    <div className="card mb-5 p-4">
      <h3 className="mb-2 flex items-center gap-2 text-sm font-semibold text-neutral-200">
        <ListOrdered size={15} className="text-indigo-400" /> Next up
        <span className="text-xs font-normal text-neutral-500">what the scheduler will run next</span>
      </h3>
      <div className="flex flex-col gap-1.5">
        {queue.map((q, i) => {
          // Effective priority above the base means aging has bumped it.
          const aged = q.effectivePriority - q.task.priority >= 1;
          return (
            <button
              key={q.task.id}
              onClick={() => navigate(`/tasks/${q.task.id}`)}
              className="flex items-center gap-3 rounded-md border border-[var(--color-border)] bg-[var(--color-surface)] px-3 py-2 text-left text-sm hover:border-indigo-500/40"
            >
              <span className="w-5 shrink-0 text-center text-[11px] text-neutral-600">{i + 1}</span>
              <span className="min-w-0 flex-1 truncate text-neutral-200">{q.task.title}</span>
              {aged && (
                <span className="shrink-0 rounded bg-amber-500/10 px-1.5 py-0.5 text-[10px] text-amber-400" title="Boosted by priority aging">
                  aged +{Math.round(q.effectivePriority - q.task.priority)}
                </span>
              )}
              <PriorityBadge priority={q.task.priority} />
              <AgentBadge agent={q.task.agent} />
              <span className="shrink-0 text-[11px] text-neutral-500">{q.projectName}</span>
            </button>
          );
        })}
      </div>
    </div>
  );
}
