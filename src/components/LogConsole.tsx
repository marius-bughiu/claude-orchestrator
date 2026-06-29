import { useEffect, useRef, useState } from "react";
import { Terminal, ChevronRight, ChevronDown, Trash2 } from "lucide-react";
import { useStore } from "../store";

const LEVEL_COLOR: Record<string, string> = {
  error: "text-rose-400",
  warn: "text-amber-400",
  info: "text-neutral-300",
  debug: "text-neutral-500",
};

/// A collapsible live console of the engine's log stream (from Log events the
/// store buffers in memory). Auto-scrolls to the newest line when open.
export function LogConsole() {
  const logs = useStore((s) => s.logs);
  const clearLogs = useStore((s) => s.clearLogs);
  const [open, setOpen] = useState(false);
  const bodyRef = useRef<HTMLDivElement>(null);

  // Logs are newest-first in the store; show oldest-first and pin to bottom.
  const ordered = [...logs].reverse();
  useEffect(() => {
    if (open && bodyRef.current) bodyRef.current.scrollTop = bodyRef.current.scrollHeight;
  }, [open, logs]);

  return (
    <div className="card mb-5 overflow-hidden p-0">
      <div className="flex items-center gap-2 px-4 py-2">
        <button onClick={() => setOpen((o) => !o)} className="flex flex-1 items-center gap-2 text-sm font-medium text-neutral-300 hover:text-neutral-100">
          {open ? <ChevronDown size={14} /> : <ChevronRight size={14} />}
          <Terminal size={14} className="text-indigo-400" /> Console
          <span className="text-[11px] text-neutral-500">{logs.length} line{logs.length === 1 ? "" : "s"}</span>
        </button>
        {open && logs.length > 0 && (
          <button onClick={clearLogs} className="flex items-center gap-1 text-[11px] text-neutral-500 hover:text-rose-400" title="Clear console">
            <Trash2 size={12} /> Clear
          </button>
        )}
      </div>
      {open && (
        <div ref={bodyRef} className="max-h-64 overflow-y-auto border-t border-[var(--color-border)] bg-[var(--color-bg)] p-2 font-mono text-[11px] leading-relaxed">
          {ordered.length === 0 && <p className="px-2 py-4 text-center text-neutral-600">No log output yet.</p>}
          {ordered.map((l, i) => (
            <div key={i} className="flex gap-2 whitespace-pre-wrap px-1">
              <span className="shrink-0 text-neutral-600">{new Date(l.ts).toLocaleTimeString()}</span>
              <span className={`shrink-0 uppercase ${LEVEL_COLOR[l.level] ?? "text-neutral-400"}`}>{l.level}</span>
              <span className="min-w-0 text-neutral-300">{l.message}</span>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
