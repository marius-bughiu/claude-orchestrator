import { useEffect, useMemo, useState } from "react";
import { ResponsiveContainer, BarChart, Bar, XAxis, YAxis, CartesianGrid, Tooltip, Legend } from "recharts";
import { Activity } from "lucide-react";
import * as api from "../api";
import type { ThroughputPoint } from "../api/types";

const AXIS = "#8b95a7";
const GRID = "rgba(130,140,160,0.18)";

/// Sessions completed vs failed per day — a view of fleet output and reliability,
/// distinct from the cost/token usage charts. Hidden until there's data.
export function ThroughputChart() {
  const [points, setPoints] = useState<ThroughputPoint[]>([]);

  useEffect(() => {
    let active = true;
    const load = () => api.sessionThroughput(14).then((p) => active && setPoints(p)).catch(() => {});
    load();
    const unlisten = api.onOrchestratorEvent((e) => {
      if (e.type === "sessionUpdated") load();
    });
    return () => { active = false; unlisten.then((u) => u()); };
  }, []);

  const { data, totalCompleted, totalFailed } = useMemo(() => {
    const data = points.map((p) => ({ label: p.date.slice(5), completed: p.completed, failed: p.failed }));
    const totalCompleted = points.reduce((a, p) => a + p.completed, 0);
    const totalFailed = points.reduce((a, p) => a + p.failed, 0);
    return { data, totalCompleted, totalFailed };
  }, [points]);

  if (points.length === 0) return null;

  const total = totalCompleted + totalFailed;
  const successPct = total > 0 ? Math.round((totalCompleted / total) * 100) : 0;
  const light = typeof document !== "undefined" && document.documentElement.classList.contains("light");
  const tooltipStyle = {
    background: light ? "#ffffff" : "#11151d",
    border: `1px solid ${light ? "#e0e4ea" : "#232a36"}`,
    borderRadius: 8,
    fontSize: 12,
    color: light ? "#161a22" : "#e6e9ef",
  };

  return (
    <div className="card mb-5 p-4">
      <div className="mb-3 flex items-center justify-between">
        <h3 className="flex items-center gap-2 text-sm font-semibold text-neutral-200">
          <Activity size={15} className="text-indigo-400" /> Throughput
          <span className="text-xs font-normal text-neutral-500">sessions per day · last 14 days</span>
        </h3>
        <div className="text-xs text-neutral-500">
          <span className="text-emerald-400">{totalCompleted} completed</span>
          {totalFailed > 0 && <span className="text-rose-400"> · {totalFailed} failed</span>}
          <span> · {successPct}% success</span>
        </div>
      </div>
      <div className="h-48 w-full">
        <ResponsiveContainer width="100%" height="100%">
          <BarChart data={data} margin={{ top: 8, right: 12, left: 0, bottom: 0 }}>
            <CartesianGrid strokeDasharray="3 3" stroke={GRID} vertical={false} />
            <XAxis dataKey="label" tick={{ fill: AXIS, fontSize: 11 }} stroke={GRID} />
            <YAxis tick={{ fill: AXIS, fontSize: 11 }} stroke={GRID} width={32} allowDecimals={false} />
            <Tooltip contentStyle={tooltipStyle} cursor={{ fill: "rgba(130,140,160,0.08)" }} />
            <Legend wrapperStyle={{ fontSize: 11 }} />
            <Bar dataKey="completed" name="completed" stackId="t" fill="#34d399" radius={[0, 0, 0, 0]} />
            <Bar dataKey="failed" name="failed" stackId="t" fill="#f87171" radius={[3, 3, 0, 0]} />
          </BarChart>
        </ResponsiveContainer>
      </div>
    </div>
  );
}
