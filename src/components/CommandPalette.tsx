import { useEffect, useMemo, useRef, useState } from "react";
import { useNavigate } from "react-router-dom";
import {
  LayoutDashboard, FolderGit2, ListTodo, Clock, GanttChartSquare, Settings as SettingsIcon,
  Play, Pause, RefreshCw, Palette, Search,
} from "lucide-react";
import { useStore } from "../store";
import * as api from "../api";
import { applyTheme, getTheme } from "../lib/theme";

interface Cmd {
  id: string;
  label: string;
  hint?: string;
  icon: typeof Search;
  run: () => void;
}

/// A Cmd/Ctrl+K command palette for fast navigation and actions.
export function CommandPalette() {
  const navigate = useNavigate();
  const status = useStore((s) => s.status);
  const refreshStatus = useStore((s) => s.refreshStatus);
  const refreshScheduled = useStore((s) => s.refreshScheduled);
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const [sel, setSel] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "k") {
        e.preventDefault();
        setOpen((o) => !o);
      } else if (e.key === "Escape") {
        setOpen(false);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  useEffect(() => {
    if (open) {
      setQuery("");
      setSel(0);
      setTimeout(() => inputRef.current?.focus(), 0);
    }
  }, [open]);

  const commands = useMemo<Cmd[]>(() => {
    const go = (to: string) => () => { navigate(to); setOpen(false); };
    const running = status?.running;
    return [
      { id: "dash", label: "Go to Dashboard", icon: LayoutDashboard, run: go("/dashboard") },
      { id: "proj", label: "Go to Projects", icon: FolderGit2, run: go("/projects") },
      { id: "tasks", label: "Go to Tasks", icon: ListTodo, run: go("/tasks") },
      { id: "sched", label: "Go to Scheduled", icon: Clock, run: go("/scheduled") },
      { id: "time", label: "Go to Timeline", icon: GanttChartSquare, run: go("/timeline") },
      { id: "set", label: "Go to Settings", icon: SettingsIcon, run: go("/settings") },
      {
        id: "run",
        label: running ? "Pause orchestrator" : "Run orchestrator",
        icon: running ? Pause : Play,
        run: async () => { await api.setRunning(!running); await refreshStatus(); setOpen(false); },
      },
      {
        id: "rescan",
        label: "Rescan scheduled tasks",
        icon: RefreshCw,
        run: async () => { await api.refreshScheduled(); await refreshScheduled(); setOpen(false); },
      },
      {
        id: "theme",
        label: "Cycle theme (dark / light / system)",
        icon: Palette,
        run: () => {
          const order = ["dark", "light", "system"] as const;
          const next = order[(order.indexOf(getTheme() as (typeof order)[number]) + 1) % order.length];
          applyTheme(next);
          setOpen(false);
        },
      },
    ];
  }, [navigate, status, refreshStatus, refreshScheduled]);

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    return q ? commands.filter((c) => c.label.toLowerCase().includes(q)) : commands;
  }, [commands, query]);

  if (!open) return null;

  const onKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "ArrowDown") { e.preventDefault(); setSel((s) => Math.min(s + 1, filtered.length - 1)); }
    else if (e.key === "ArrowUp") { e.preventDefault(); setSel((s) => Math.max(s - 1, 0)); }
    else if (e.key === "Enter") { e.preventDefault(); filtered[sel]?.run(); }
  };

  return (
    <div className="fixed inset-0 z-[60] flex items-start justify-center bg-black/50 p-6 pt-[14vh]" onMouseDown={() => setOpen(false)}>
      <div className="card w-full max-w-lg overflow-hidden shadow-2xl" onMouseDown={(e) => e.stopPropagation()}>
        <div className="flex items-center gap-2 border-b border-[var(--color-border)] px-3 py-2">
          <Search size={15} className="text-neutral-500" />
          <input
            ref={inputRef}
            className="w-full bg-transparent text-sm text-neutral-100 outline-none placeholder:text-neutral-500"
            placeholder="Type a command…"
            value={query}
            onChange={(e) => { setQuery(e.target.value); setSel(0); }}
            onKeyDown={onKeyDown}
          />
          <kbd className="rounded border border-[var(--color-border)] px-1.5 text-[10px] text-neutral-500">esc</kbd>
        </div>
        <div className="max-h-80 overflow-y-auto py-1">
          {filtered.length === 0 && <div className="px-3 py-6 text-center text-sm text-neutral-500">No commands</div>}
          {filtered.map((c, i) => {
            const Icon = c.icon;
            return (
              <button
                key={c.id}
                onMouseEnter={() => setSel(i)}
                onClick={c.run}
                className={`flex w-full items-center gap-2.5 px-3 py-2 text-left text-sm ${i === sel ? "bg-indigo-600/15 text-indigo-100" : "text-neutral-300"}`}
              >
                <Icon size={15} className="text-neutral-400" />
                {c.label}
              </button>
            );
          })}
        </div>
      </div>
    </div>
  );
}
