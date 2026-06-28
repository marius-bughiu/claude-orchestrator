import clsx from "clsx";
import { Pause, Play, Activity, ListTodo } from "lucide-react";
import { useStore } from "../store";
import * as api from "../api";
import type { AgentUsage, WindowUsage } from "../api/types";
import { AGENT_COLORS, AGENT_LABELS, formatCost, formatTokens } from "../lib/format";

function pctColor(pct: number): string {
  if (pct > 0.9) return "bg-red-500";
  if (pct > 0.7) return "bg-amber-500";
  return "bg-indigo-500";
}

/// A labeled usage meter showing percent-of-limit, with a thin progress bar.
function Meter({ label, win }: { label: string; win: WindowUsage }) {
  const pct = win.costPct;
  return (
    <div className="flex items-center gap-1.5">
      <span className="w-3.5 shrink-0 text-[10px] font-medium uppercase text-neutral-500">{label}</span>
      {pct === null ? (
        <span className="text-[10px] text-neutral-600">no limit</span>
      ) : (
        <>
          <div className="h-1 flex-1 overflow-hidden rounded-full bg-neutral-700/50">
            <div
              className={clsx("h-full rounded-full transition-all", pctColor(pct))}
              style={{ width: `${Math.min(100, pct * 100)}%` }}
            />
          </div>
          <span
            className={clsx(
              "w-8 shrink-0 text-right text-[10px] font-semibold tabular-nums",
              pct > 0.9 ? "text-red-300" : pct > 0.7 ? "text-amber-300" : "text-neutral-300",
            )}
          >
            {Math.round(pct * 100)}%
          </span>
        </>
      )}
    </div>
  );
}

function AgentCard({ usage }: { usage: AgentUsage }) {
  const s = usage.session.usage;
  const tokens = s.inputTokens + s.outputTokens + s.cacheReadTokens;
  const hasLimits = usage.session.costPct !== null || usage.weekly.costPct !== null;

  return (
    <div className="flex min-w-[190px] flex-col gap-1 rounded-md border border-[var(--color-border)] bg-[var(--color-surface-2)] px-3 py-2">
      <div className="flex items-center justify-between">
        <span className={clsx("chip", AGENT_COLORS[usage.agent])}>{AGENT_LABELS[usage.agent]}</span>
        <span
          className={clsx("h-2 w-2 rounded-full", usage.available ? "bg-emerald-400" : "bg-neutral-600")}
          title={usage.available ? "CLI detected" : "CLI not found on PATH"}
        />
      </div>
      <div className="flex items-baseline justify-between text-xs">
        <span className="font-semibold text-neutral-100">{formatCost(s.totalCostUsd)}</span>
        <span className="text-neutral-500">
          {formatTokens(tokens)} tok
          {usage.activeSessions > 0 && <span className="ml-1.5 text-indigo-300">· {usage.activeSessions} live</span>}
        </span>
      </div>
      {hasLimits ? (
        <div className="mt-0.5 flex flex-col gap-1">
          <Meter label="S" win={usage.session} />
          <Meter label="W" win={usage.weekly} />
        </div>
      ) : (
        <div className="text-[10px] text-neutral-600">
          {usage.session.windowHours}h / {Math.round(usage.weekly.windowHours / 24)}d windows · no limits set
        </div>
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
        disabled={!status || status.draining}
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
        {status?.draining && <span className="text-amber-300">draining…</span>}
      </div>

      <div className="ml-auto flex items-center gap-2 overflow-x-auto">
        {status?.agents.map((a) => <AgentCard key={a.agent} usage={a} />)}
      </div>
    </header>
  );
}
