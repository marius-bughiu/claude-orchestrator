import { useEffect, useState } from "react";
import { BarChart3 } from "lucide-react";
import * as api from "../api";
import type { ProjectAnalytics } from "../api/types";
import { formatCost, formatTokens, AGENT_LABELS } from "../lib/format";

function dur(secs: number): string {
  if (secs <= 0) return "—";
  if (secs < 60) return `${Math.round(secs)}s`;
  const m = Math.floor(secs / 60);
  return m < 60 ? `${m}m ${Math.round(secs % 60)}s` : `${Math.floor(m / 60)}h ${m % 60}m`;
}

function Tile({ label, value, sub }: { label: string; value: string; sub?: string }) {
  return (
    <div className="rounded-md border border-[var(--color-border)] bg-[var(--color-surface)] px-3 py-2">
      <div className="text-[11px] text-neutral-500">{label}</div>
      <div className="text-base font-semibold text-neutral-100">{value}</div>
      {sub && <div className="text-[11px] text-neutral-600">{sub}</div>}
    </div>
  );
}

/// Per-project analytics: headline totals, a per-agent breakdown, and a compact
/// completed/failed-per-day strip. Hidden until the project has finished work.
export function ProjectAnalyticsPanel({ projectId }: { projectId: string }) {
  const [data, setData] = useState<ProjectAnalytics | null>(null);

  useEffect(() => {
    let active = true;
    const load = () => api.projectAnalytics(projectId, 14).then((a) => active && setData(a)).catch(() => {});
    load();
    const unlisten = api.onOrchestratorEvent((e) => {
      if (e.type === "sessionUpdated") load();
    });
    return () => { active = false; unlisten.then((u) => u()); };
  }, [projectId]);

  if (!data || data.stats.sessions === 0) return null;
  const s = data.stats;
  const maxDay = Math.max(1, ...data.throughput.map((t) => t.completed + t.failed));

  return (
    <div className="card p-4">
      <h3 className="mb-3 flex items-center gap-2 text-sm font-semibold text-neutral-200">
        <BarChart3 size={15} className="text-indigo-400" /> Analytics
        <span className="text-xs font-normal text-neutral-500">finished task sessions · last 14 days</span>
      </h3>

      <div className="mb-4 grid grid-cols-2 gap-2 sm:grid-cols-4">
        <Tile label="Sessions" value={String(s.sessions)} sub={`${s.completed} ok · ${s.failed} failed`} />
        <Tile label="Success" value={`${Math.round(s.successRate * 100)}%`} />
        <Tile label="Total cost" value={formatCost(s.totalCostUsd)} sub={`${formatTokens(s.totalTokens)} tokens`} />
        <Tile label="Avg duration" value={dur(s.avgDurationSecs)} />
      </div>

      {data.throughput.length > 0 && (
        <div className="mb-4">
          <div className="mb-1 text-[11px] text-neutral-500">Throughput</div>
          <div className="flex h-16 items-end gap-1">
            {data.throughput.map((t) => {
              const total = t.completed + t.failed;
              return (
                <div key={t.date} className="flex flex-1 flex-col justify-end" title={`${t.date}: ${t.completed} ok, ${t.failed} failed`}>
                  <div className="w-full rounded-sm bg-rose-500/80" style={{ height: `${(t.failed / maxDay) * 56}px` }} />
                  <div className="w-full rounded-sm bg-emerald-500/80" style={{ height: `${(t.completed / maxDay) * 56}px` }} />
                  {total === 0 && <div className="h-px w-full bg-[var(--color-border)]" />}
                </div>
              );
            })}
          </div>
        </div>
      )}

      {data.byAgent.length > 0 && (
        <div className="flex flex-col gap-1.5">
          {data.byAgent.map((a) => (
            <div key={a.agent} className="flex items-center gap-3 text-sm">
              <span className="w-20 shrink-0 text-neutral-300">{AGENT_LABELS[a.agent]}</span>
              <div className="h-1.5 flex-1 overflow-hidden rounded-full bg-[var(--color-border)]">
                <div
                  className={`h-full ${a.successRate >= 0.7 ? "bg-emerald-500" : a.successRate >= 0.4 ? "bg-amber-500" : "bg-rose-500"}`}
                  style={{ width: `${Math.min(1, a.successRate) * 100}%` }}
                />
              </div>
              <span className="w-12 shrink-0 text-right text-neutral-300">{Math.round(a.successRate * 100)}%</span>
              <span className="w-24 shrink-0 text-right text-[11px] text-neutral-500">{a.sessions} sess · {formatCost(a.totalCostUsd)}</span>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
