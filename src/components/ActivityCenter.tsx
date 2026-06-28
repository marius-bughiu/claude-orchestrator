import { useEffect, useRef, useState } from "react";
import clsx from "clsx";
import { Bell, CheckCircle2, AlertTriangle, Clock, Info } from "lucide-react";
import { useStore } from "../store";
import type { ActivityItem } from "../store";
import { formatRelative } from "../lib/format";

function ItemIcon({ item }: { item: ActivityItem }) {
  if (item.kind === "task")
    return item.level === "error"
      ? <AlertTriangle size={14} className="text-red-400" />
      : <CheckCircle2 size={14} className="text-emerald-400" />;
  if (item.kind === "scheduled") return <Clock size={14} className="text-blue-400" />;
  if (item.level === "error") return <AlertTriangle size={14} className="text-red-400" />;
  if (item.level === "warn") return <AlertTriangle size={14} className="text-amber-400" />;
  return <Info size={14} className="text-neutral-500" />;
}

/// A bell with an unread badge and a popover feed of recent activity.
export function ActivityCenter() {
  const activity = useStore((s) => s.activity);
  const unread = useStore((s) => s.unread);
  const markRead = useStore((s) => s.markActivityRead);
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    markRead();
    const onClick = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) setOpen(false);
    };
    window.addEventListener("mousedown", onClick);
    return () => window.removeEventListener("mousedown", onClick);
  }, [open, markRead]);

  return (
    <div className="relative" ref={ref}>
      <button
        className="relative flex h-8 w-8 items-center justify-center rounded-md text-neutral-400 hover:bg-[var(--color-surface-2)] hover:text-neutral-200"
        onClick={() => setOpen((o) => !o)}
        title="Activity"
        aria-label="Activity"
      >
        <Bell size={16} />
        {unread > 0 && (
          <span className="absolute -right-0.5 -top-0.5 flex h-4 min-w-4 items-center justify-center rounded-full bg-indigo-500 px-1 text-[9px] font-semibold text-white">
            {unread > 99 ? "99+" : unread}
          </span>
        )}
      </button>

      {open && (
        <div className="absolute left-0 top-10 z-50 w-80 overflow-hidden rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)] shadow-2xl">
          <div className="border-b border-[var(--color-border)] px-3 py-2 text-xs font-semibold text-neutral-300">
            Activity
          </div>
          <div className="max-h-96 overflow-y-auto">
            {activity.length === 0 ? (
              <div className="px-3 py-8 text-center text-sm text-neutral-500">No activity yet.</div>
            ) : (
              activity.map((it) => (
                <div key={it.id} className={clsx("flex items-start gap-2 px-3 py-2 text-xs", "border-b border-[var(--color-border)] last:border-0")}>
                  <span className="mt-0.5 shrink-0"><ItemIcon item={it} /></span>
                  <span className="min-w-0 flex-1 break-words text-neutral-300">{it.message}</span>
                  <span className="shrink-0 text-[10px] text-neutral-600">{formatRelative(new Date(it.ts).toISOString())}</span>
                </div>
              ))
            )}
          </div>
        </div>
      )}
    </div>
  );
}
