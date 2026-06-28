import { useMemo, useState } from "react";
import { AlertTriangle, X } from "lucide-react";
import { useStore } from "../store";
import { AGENT_LABELS } from "../lib/format";

const THRESHOLD = 0.8; // warn at 80% of a configured limit

/// A non-blocking banner that warns when an agent nears a configured usage limit.
/// Monitoring only — it never pauses or blocks work.
export function UsageAlertBanner() {
  const status = useStore((s) => s.status);
  const [dismissed, setDismissed] = useState<string>("");

  const alerts = useMemo(() => {
    const out: { label: string; window: string; pct: number }[] = [];
    for (const a of status?.agents ?? []) {
      const checks: [string, number | null][] = [
        ["session", a.session.costPct],
        ["weekly", a.weekly.costPct],
      ];
      for (const [w, pct] of checks) {
        if (pct !== null && pct >= THRESHOLD) {
          out.push({ label: AGENT_LABELS[a.agent], window: w, pct });
        }
      }
    }
    return out.sort((x, y) => y.pct - x.pct);
  }, [status]);

  // Signature changes when the set/severity of alerts changes → re-show.
  const signature = alerts.map((a) => `${a.label}:${a.window}:${Math.round(a.pct * 100)}`).join("|");
  if (alerts.length === 0 || dismissed === signature) return null;

  const top = alerts[0];
  const over = top.pct >= 1;

  return (
    <div
      className={`flex items-center gap-2 border-b px-4 py-2 text-sm ${
        over ? "border-red-500/30 bg-red-500/10 text-red-200" : "border-amber-500/30 bg-amber-500/10 text-amber-100"
      }`}
    >
      <AlertTriangle size={16} className="shrink-0" />
      <span>
        <span className="font-semibold">{top.label}</span> is at{" "}
        <span className="font-semibold">{Math.round(top.pct * 100)}%</span> of its {top.window} cost limit
        {alerts.length > 1 && <span className="opacity-80"> (+{alerts.length - 1} more)</span>}.
      </span>
      <button className="ml-auto opacity-80 hover:opacity-100" onClick={() => setDismissed(signature)} title="Dismiss">
        <X size={16} />
      </button>
    </div>
  );
}
