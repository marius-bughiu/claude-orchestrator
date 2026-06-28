import clsx from "clsx";
import { Pause, Play, Activity, ListTodo } from "lucide-react";
import { useStore } from "../store";
import * as api from "../api";
import type { AgentUsage } from "../api/types";
import { AGENT_COLORS, AGENT_LABELS, formatCost, formatTokens } from "../lib/format";

function AgentCard({ usage }: { usage: AgentUsage }) {
  const cost = usage.window.totalCostUsd;
  const tokens =
    usage.window.inputTokens + usage.window.outputTokens + usage.window.cacheReadTokens;
  const limit = usage.limits.costLimitUsd;
  const pct = limit ? Math.min(100, (cost / limit) * 100) : null;

  return (
    <div className="flex min-w-[150px] flex-col gap-1 rounded-md border border-[var(--color-border)] bg-[var(--color-surface-2)] px-3 py-2">
      <div className="flex items-center justify-between">
        <span className={clsx("chip", AGENT_COLORS[usage.agent])}>
          {AGENT_LABELS[usage.agent]}
        </span>
        <span
          className={clsx(
            "h-2 w-2 rounded-full",
            usage.available ? "bg-emerald-400" : "bg-neutral-600",
          )}
          title={usage.available ? "CLI detected" : "CLI not found on PATH"}
        />
      </div>
      <div className="flex items-baseline justify-between text-xs">
        <span className="font-semibold text-neutral-100">{formatCost(cost)}</span>
        <span className="text-neutral-500">{formatTokens(tokens)} tok</span>
      </div>
      {pct !== null ? (
        <div className="h-1 w-full overflow-hidden rounded-full bg-neutral-700/50">
          <div
            className={clsx(
              "h-full rounded-full transition-all",
              pct > 90 ? "bg-red-500" : pct > 70 ? "bg-amber-500" : "bg-indigo-500",
            )}
            style={{ width: `${pct}%` }}
          />
        </div>
      ) : (
        <div className="text-[10px] text-neutral-600">no limit set · {usage.windowHours}h window</div>
      )}
      {usage.activeSessions > 0 && (
        <div className="text-[10px] text-indigo-300">{usage.activeSessions} active</div>
      )}
    </div>
  );
}

export function UsageBar() {
  const status = useStore((s) => s.status);
  const refreshStatus = useStore((s) => s.refreshStatus);

  const toggleRunning = async () => {
    if (!status) return;
    await api.setRunning(!status.running);
    await refreshStatus();
  };

  return (
    <header className="flex items-center gap-3 border-b border-[var(--color-border)] bg-[var(--color-surface)] px-4 py-2.5">
      <button
        className={clsx("btn", status?.running ? "btn-danger" : "btn-primary")}
        onClick={toggleRunning}
        disabled={!status}
      >
        {status?.running ? <Pause size={15} /> : <Play size={15} />}
        {status?.running ? "Pause" : "Run"}
      </button>

      <div className="flex items-center gap-4 text-xs text-neutral-400">
        <span className="flex items-center gap-1.5">
          <Activity size={14} className={status?.running ? "text-emerald-400" : "text-neutral-500"} />
          <span className="text-neutral-200">{status?.activeSessions ?? 0}</span>/
          {status?.maxConcurrent ?? 0} active
        </span>
        <span className="flex items-center gap-1.5">
          <ListTodo size={14} className="text-neutral-500" />
          <span className="text-neutral-200">{status?.pendingTasks ?? 0}</span> pending
        </span>
      </div>

      <div className="ml-auto flex items-center gap-2 overflow-x-auto">
        {status?.agents.map((a) => <AgentCard key={a.agent} usage={a} />)}
      </div>
    </header>
  );
}
