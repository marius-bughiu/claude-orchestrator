import { useMemo } from "react";
import { TrendingUp } from "lucide-react";
import { useStore } from "../store";
import type { AgentUsage, WindowUsage } from "../api/types";
import { formatCost, AGENT_LABELS } from "../lib/format";

/// Project a window's end-of-period spend from the burn rate so far. Returns null
/// until enough of the window has elapsed for the extrapolation to be meaningful.
function project(w: WindowUsage): { projected: number; spent: number } | null {
  if (!w.windowStartedAt || w.usage.totalCostUsd <= 0) return null;
  const elapsedH = (Date.now() - new Date(w.windowStartedAt).getTime()) / 3.6e6;
  if (elapsedH < 0.5 || elapsedH >= w.windowHours) return null; // too early / window over
  const projected = (w.usage.totalCostUsd / elapsedH) * w.windowHours;
  return { projected, spent: w.usage.totalCostUsd };
}

/// Monitoring-only forecast: extrapolates each agent's current weekly burn rate
/// to the end of its window. Never enforces — limits remain advisory. Hidden
/// until at least one agent has a usable projection.
export function CostForecast() {
  const status = useStore((s) => s.status);

  const rows = useMemo(() => {
    return (status?.agents ?? [])
      .map((a: AgentUsage) => ({ agent: a.agent, limit: a.weekly.costLimitUsd, p: project(a.weekly) }))
      .filter((r): r is { agent: AgentUsage["agent"]; limit: number | null; p: { projected: number; spent: number } } => r.p !== null);
  }, [status]);

  if (rows.length === 0) return null;

  return (
    <div className="card mb-5 p-4">
      <h3 className="mb-3 flex items-center gap-2 text-sm font-semibold text-neutral-200">
        <TrendingUp size={15} className="text-indigo-400" /> Cost forecast
        <span className="text-xs font-normal text-neutral-500">projected weekly spend at the current rate</span>
      </h3>
      <div className="flex flex-col gap-2.5">
        {rows.map(({ agent, limit, p }) => {
          const overLimit = limit != null && p.projected > limit;
          const pct = limit != null && limit > 0 ? Math.min(1, p.projected / limit) : null;
          return (
            <div key={agent} className="text-sm">
              <div className="mb-1 flex items-center justify-between">
                <span className="text-neutral-300">{AGENT_LABELS[agent]}</span>
                <span className="text-neutral-400">
                  {formatCost(p.spent)} so far ·{" "}
                  <span className={overLimit ? "text-rose-400" : "text-neutral-200"}>
                    ~{formatCost(p.projected)} projected
                  </span>
                  {limit != null && <span className="text-neutral-600"> / {formatCost(limit)}</span>}
                </span>
              </div>
              {pct != null && (
                <div className="h-1.5 overflow-hidden rounded-full bg-[var(--color-border)]">
                  <div
                    className={`h-full ${overLimit ? "bg-rose-500" : pct >= 0.8 ? "bg-amber-500" : "bg-indigo-500"}`}
                    style={{ width: `${pct * 100}%` }}
                  />
                </div>
              )}
            </div>
          );
        })}
      </div>
    </div>
  );
}
