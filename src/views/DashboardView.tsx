import { useEffect, useMemo, useState } from "react";
import {
  ResponsiveContainer, AreaChart, Area, BarChart, Bar, XAxis, YAxis,
  CartesianGrid, Tooltip, Legend,
} from "recharts";
import { DollarSign, Coins, PlayCircle, Download } from "lucide-react";
import * as api from "../api";
import type { AgentKind, UsagePoint } from "../api/types";
import { useStore } from "../store";
import { formatCost, formatTokens, AGENT_LABELS } from "../lib/format";
import { AgentComparison } from "../components/AgentComparison";
import { NeedsAttention } from "../components/NeedsAttention";

type Gran = "day" | "month" | "year";
const GRANS: { label: string; value: Gran; limit: number }[] = [
  { label: "Daily", value: "day", limit: 30 },
  { label: "Monthly", value: "month", limit: 24 },
  { label: "Yearly", value: "year", limit: 10 },
];
const AGENTS: (AgentKind | "all")[] = ["all", "claude", "gemini", "codex"];

const AXIS = "#8b95a7";
const GRID = "rgba(130,140,160,0.18)";

function tooltipStyleFor(light: boolean) {
  return {
    background: light ? "#ffffff" : "#11151d",
    border: `1px solid ${light ? "#e0e4ea" : "#232a36"}`,
    borderRadius: 8,
    fontSize: 12,
    color: light ? "#161a22" : "#e6e9ef",
  };
}

function periodLabel(period: string, gran: Gran): string {
  if (gran === "day") return period.slice(5); // MM-DD
  return period;
}

function StatCard({ icon, label, value, sub }: { icon: React.ReactNode; label: string; value: string; sub?: string }) {
  return (
    <div className="card flex items-center gap-3 p-4">
      <div className="text-indigo-400">{icon}</div>
      <div>
        <div className="text-xs text-neutral-500">{label}</div>
        <div className="text-lg font-semibold text-neutral-100">{value}</div>
        {sub && <div className="text-[11px] text-neutral-600">{sub}</div>}
      </div>
    </div>
  );
}

function ChartCard({ title, children }: { title: string; children: React.ReactElement }) {
  return (
    <div className="card p-4">
      <h3 className="mb-3 text-sm font-semibold text-neutral-200">{title}</h3>
      <div className="h-64 w-full">
        <ResponsiveContainer width="100%" height="100%">{children}</ResponsiveContainer>
      </div>
    </div>
  );
}

export function DashboardView() {
  const refreshStatus = useStore((s) => s.refreshStatus);
  const [gran, setGran] = useState<Gran>("day");
  const [agent, setAgent] = useState<AgentKind | "all">("all");
  const [series, setSeries] = useState<UsagePoint[]>([]);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    let active = true;
    setLoading(true);
    const conf = GRANS.find((g) => g.value === gran)!;
    api
      .usageSeries(gran, agent === "all" ? undefined : agent, conf.limit)
      .then((r) => { if (active) { setSeries(r); setLoading(false); } })
      .catch(() => active && setLoading(false));
    const unlisten = api.onOrchestratorEvent((e) => {
      if (e.type === "usageUpdated" || e.type === "sessionUpdated") {
        api.usageSeries(gran, agent === "all" ? undefined : agent, conf.limit).then((r) => active && setSeries(r)).catch(() => {});
      }
    });
    return () => { active = false; unlisten.then((u) => u()); };
  }, [gran, agent, refreshStatus]);

  const data = useMemo(
    () =>
      series.map((p) => ({
        ...p,
        label: periodLabel(p.period, gran),
        cost: Number(p.costUsd.toFixed(4)),
        cacheTokens: p.cacheReadTokens + p.cacheCreationTokens,
      })),
    [series, gran],
  );

  const totals = useMemo(() => {
    const cost = series.reduce((a, p) => a + p.costUsd, 0);
    const tokens = series.reduce((a, p) => a + p.totalTokens, 0);
    const sessions = series.reduce((a, p) => a + p.sessions, 0);
    return { cost, tokens, sessions };
  }, [series]);

  const tooltipStyle = tooltipStyleFor(
    typeof document !== "undefined" && document.documentElement.classList.contains("light"),
  );

  const exportCsv = () => {
    const cols = ["period", "inputTokens", "outputTokens", "cacheReadTokens", "cacheCreationTokens", "totalTokens", "costUsd", "numTurns", "sessions"] as const;
    const rows = [cols.join(","), ...series.map((p) => cols.map((c) => p[c]).join(","))];
    const blob = new Blob([rows.join("\n")], { type: "text/csv" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = `orchestrator-usage-${gran}-${agent}.csv`;
    a.click();
    URL.revokeObjectURL(url);
  };

  return (
    <div className="p-6">
      <div className="mb-5 flex flex-wrap items-end justify-between gap-3">
        <div>
          <h1 className="text-lg font-semibold text-neutral-100">Dashboard</h1>
          <p className="text-xs text-neutral-500">Usage, tokens, and sessions over time.</p>
        </div>
        <div className="flex items-center gap-2">
          <div className="flex gap-1">
            {GRANS.map((g) => (
              <button
                key={g.value}
                className={`chip border ${gran === g.value ? "border-indigo-500/50 bg-indigo-600/15 text-indigo-200" : "border-[var(--color-border)] text-neutral-400 hover:text-neutral-200"}`}
                onClick={() => setGran(g.value)}
              >
                {g.label}
              </button>
            ))}
          </div>
          <select className="input max-w-[140px]" value={agent} onChange={(e) => setAgent(e.target.value as AgentKind | "all")}>
            {AGENTS.map((a) => (
              <option key={a} value={a}>{a === "all" ? "All agents" : AGENT_LABELS[a as AgentKind]}</option>
            ))}
          </select>
          <button className="btn" onClick={exportCsv} disabled={series.length === 0} title="Export current series as CSV">
            <Download size={14} /> CSV
          </button>
        </div>
      </div>

      <div className="mb-5 grid grid-cols-1 gap-3 sm:grid-cols-3">
        <StatCard icon={<DollarSign size={20} />} label="Total cost" value={formatCost(totals.cost)} sub={`${data.length} ${gran}s`} />
        <StatCard icon={<Coins size={20} />} label="Total tokens" value={formatTokens(totals.tokens)} />
        <StatCard icon={<PlayCircle size={20} />} label="Sessions" value={totals.sessions.toLocaleString()} />
      </div>

      <NeedsAttention />
      <AgentComparison />

      {loading ? (
        <div className="py-16 text-center text-sm text-neutral-500">Loading…</div>
      ) : data.length === 0 ? (
        <div className="rounded-lg border border-dashed border-[var(--color-border)] py-16 text-center text-sm text-neutral-500">
          No usage recorded yet. Run some tasks and the charts will fill in.
        </div>
      ) : (
        <div className="grid grid-cols-1 gap-4 xl:grid-cols-2">
          <ChartCard title="Cost (USD)">
            <AreaChart data={data} margin={{ top: 8, right: 12, left: 0, bottom: 0 }}>
              <defs>
                <linearGradient id="costFill" x1="0" y1="0" x2="0" y2="1">
                  <stop offset="0%" stopColor="#6366f1" stopOpacity={0.5} />
                  <stop offset="100%" stopColor="#6366f1" stopOpacity={0.03} />
                </linearGradient>
              </defs>
              <CartesianGrid strokeDasharray="3 3" stroke={GRID} vertical={false} />
              <XAxis dataKey="label" tick={{ fill: AXIS, fontSize: 11 }} stroke={GRID} />
              <YAxis tick={{ fill: AXIS, fontSize: 11 }} stroke={GRID} width={48} tickFormatter={(v) => `$${v}`} />
              <Tooltip contentStyle={tooltipStyle} formatter={(v: any) => [formatCost(Number(v)), "cost"]} />
              <Area type="monotone" dataKey="cost" stroke="#818cf8" strokeWidth={2} fill="url(#costFill)" />
            </AreaChart>
          </ChartCard>

          <ChartCard title="Tokens">
            <BarChart data={data} margin={{ top: 8, right: 12, left: 0, bottom: 0 }}>
              <CartesianGrid strokeDasharray="3 3" stroke={GRID} vertical={false} />
              <XAxis dataKey="label" tick={{ fill: AXIS, fontSize: 11 }} stroke={GRID} />
              <YAxis tick={{ fill: AXIS, fontSize: 11 }} stroke={GRID} width={48} tickFormatter={(v) => formatTokens(v)} />
              <Tooltip contentStyle={tooltipStyle} formatter={(v: any, n: any) => [formatTokens(Number(v)), n]} />
              <Legend wrapperStyle={{ fontSize: 11 }} />
              <Bar dataKey="inputTokens" name="input" stackId="t" fill="#6366f1" radius={[0, 0, 0, 0]} />
              <Bar dataKey="outputTokens" name="output" stackId="t" fill="#22d3ee" />
              <Bar dataKey="cacheTokens" name="cache" stackId="t" fill="#34d399" radius={[3, 3, 0, 0]} />
            </BarChart>
          </ChartCard>

          <ChartCard title="Sessions">
            <BarChart data={data} margin={{ top: 8, right: 12, left: 0, bottom: 0 }}>
              <CartesianGrid strokeDasharray="3 3" stroke={GRID} vertical={false} />
              <XAxis dataKey="label" tick={{ fill: AXIS, fontSize: 11 }} stroke={GRID} />
              <YAxis tick={{ fill: AXIS, fontSize: 11 }} stroke={GRID} width={36} allowDecimals={false} />
              <Tooltip contentStyle={tooltipStyle} formatter={(v: any) => [v, "sessions"]} />
              <Bar dataKey="sessions" name="sessions" fill="#a78bfa" radius={[3, 3, 0, 0]} />
            </BarChart>
          </ChartCard>

          <ChartCard title="Turns">
            <BarChart data={data} margin={{ top: 8, right: 12, left: 0, bottom: 0 }}>
              <CartesianGrid strokeDasharray="3 3" stroke={GRID} vertical={false} />
              <XAxis dataKey="label" tick={{ fill: AXIS, fontSize: 11 }} stroke={GRID} />
              <YAxis tick={{ fill: AXIS, fontSize: 11 }} stroke={GRID} width={36} allowDecimals={false} />
              <Tooltip contentStyle={tooltipStyle} formatter={(v: any) => [v, "turns"]} />
              <Bar dataKey="numTurns" name="turns" fill="#f59e0b" radius={[3, 3, 0, 0]} />
            </BarChart>
          </ChartCard>
        </div>
      )}
    </div>
  );
}
