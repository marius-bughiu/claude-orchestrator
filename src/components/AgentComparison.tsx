import { useEffect, useState } from "react";
import * as api from "../api";
import type { AgentStat } from "../api/types";
import { AGENT_LABELS, formatCost } from "../lib/format";

function pct(v: number): string {
  return `${Math.round(v * 100)}%`;
}

function duration(secs: number): string {
  if (secs <= 0) return "—";
  if (secs < 60) return `${Math.round(secs)}s`;
  const m = Math.floor(secs / 60);
  const s = Math.round(secs % 60);
  return `${m}m ${s}s`;
}

/// Side-by-side reliability/cost/speed comparison across agents. Hidden until
/// at least one agent has finished sessions, so it stays out of the way early.
export function AgentComparison() {
  const [stats, setStats] = useState<AgentStat[]>([]);

  useEffect(() => {
    let active = true;
    const load = () => api.agentStats().then((r) => active && setStats(r)).catch(() => {});
    load();
    const unlisten = api.onOrchestratorEvent((e) => {
      if (e.type === "sessionUpdated" || e.type === "usageUpdated") load();
    });
    return () => { active = false; unlisten.then((u) => u()); };
  }, []);

  const active = stats.filter((s) => s.sessions > 0);
  if (active.length === 0) return null;

  return (
    <div className="card mb-5 p-4">
      <h3 className="mb-1 text-sm font-semibold text-neutral-200">Agent comparison</h3>
      <p className="mb-3 text-xs text-neutral-500">Reliability, cost, and speed across finished task sessions.</p>
      <div className="overflow-x-auto">
        <table className="w-full text-sm">
          <thead>
            <tr className="text-left text-[11px] uppercase tracking-wide text-neutral-500">
              <th className="px-2 py-1.5 font-medium">Agent</th>
              <th className="px-2 py-1.5 font-medium">Sessions</th>
              <th className="px-2 py-1.5 font-medium">Success</th>
              <th className="px-2 py-1.5 font-medium">Avg cost</th>
              <th className="px-2 py-1.5 font-medium">Total cost</th>
              <th className="px-2 py-1.5 font-medium">Avg duration</th>
            </tr>
          </thead>
          <tbody>
            {active.map((s) => (
              <tr key={s.agent} className="border-t border-[var(--color-border)]">
                <td className="px-2 py-2 font-medium text-neutral-200">{AGENT_LABELS[s.agent]}</td>
                <td className="px-2 py-2 text-neutral-400">{s.sessions}</td>
                <td className="px-2 py-2">
                  <div className="flex items-center gap-2">
                    <div className="h-1.5 w-16 overflow-hidden rounded-full bg-[var(--color-border)]">
                      <div
                        className={`h-full ${s.successRate >= 0.7 ? "bg-emerald-500" : s.successRate >= 0.4 ? "bg-amber-500" : "bg-rose-500"}`}
                        style={{ width: pct(Math.min(1, s.successRate)) }}
                      />
                    </div>
                    <span className="text-neutral-300">{pct(s.successRate)}</span>
                    <span className="text-[11px] text-neutral-600">({s.completed}/{s.sessions})</span>
                  </div>
                </td>
                <td className="px-2 py-2 text-neutral-400">{formatCost(s.avgCostUsd)}</td>
                <td className="px-2 py-2 text-neutral-400">{formatCost(s.totalCostUsd)}</td>
                <td className="px-2 py-2 text-neutral-400">{duration(s.avgDurationSecs)}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </div>
  );
}
