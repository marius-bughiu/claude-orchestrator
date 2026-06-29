import { useEffect, useMemo, useRef, useState } from "react";
import { Link } from "react-router-dom";
import { Search } from "lucide-react";
import * as api from "../api";
import { useStore } from "../store";
import type { SessionMatch } from "../api/types";
import { AgentBadge, SessionKindBadge, SessionStatusBadge } from "../components/Badges";
import { formatRelative } from "../lib/format";

const MATCH_LABELS: Record<string, string> = {
  result: "in result",
  prompt: "in prompt",
  error: "in error",
  transcript: "in transcript",
};

/// Full-text search across session prompts, results, errors, and transcripts.
export function SearchView() {
  const projects = useStore((s) => s.projects);
  const [query, setQuery] = useState("");
  const [projectFilter, setProjectFilter] = useState("all");
  const [results, setResults] = useState<SessionMatch[] | null>(null);
  const [busy, setBusy] = useState(false);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => { inputRef.current?.focus(); }, []);

  // Debounced search as the user types.
  useEffect(() => {
    const q = query.trim();
    if (!q) { setResults(null); return; }
    setBusy(true);
    const handle = setTimeout(() => {
      api.searchSessions(q, projectFilter === "all" ? undefined : projectFilter)
        .then(setResults)
        .catch(() => setResults([]))
        .finally(() => setBusy(false));
    }, 250);
    return () => clearTimeout(handle);
  }, [query, projectFilter]);

  const projectName = useMemo(
    () => (id: string) => projects.find((p) => p.id === id)?.name ?? id,
    [projects],
  );

  return (
    <div className="p-6">
      <div className="mb-5">
        <h1 className="text-lg font-semibold text-neutral-100">Search</h1>
        <p className="text-xs text-neutral-500">Find past sessions by what the agent did — across prompts, results, and transcripts.</p>
      </div>

      <div className="mb-4 flex flex-wrap items-center gap-2">
        <div className="relative max-w-[360px] flex-1">
          <Search size={14} className="pointer-events-none absolute left-2.5 top-1/2 -translate-y-1/2 text-neutral-500" />
          <input
            ref={inputRef}
            className="input pl-8"
            placeholder="Search session content…"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
          />
        </div>
        <select className="input max-w-[200px]" value={projectFilter} onChange={(e) => setProjectFilter(e.target.value)}>
          <option value="all">All projects</option>
          {projects.map((p) => (
            <option key={p.id} value={p.id}>{p.name}</option>
          ))}
        </select>
        {results && <span className="text-xs text-neutral-500">{results.length} match{results.length === 1 ? "" : "es"}</span>}
      </div>

      {results === null ? (
        <div className="rounded-lg border border-dashed border-[var(--color-border)] py-16 text-center text-sm text-neutral-500">
          {busy ? "Searching…" : "Type to search session history."}
        </div>
      ) : results.length === 0 ? (
        <div className="rounded-lg border border-dashed border-[var(--color-border)] py-16 text-center text-sm text-neutral-500">
          No sessions match “{query.trim()}”.
        </div>
      ) : (
        <div className="flex flex-col gap-2">
          {results.map((m) => (
            <Link
              key={m.session.id}
              to={`/sessions/${m.session.id}`}
              className="card flex flex-col gap-1.5 p-3 hover:border-indigo-500/40"
            >
              <div className="flex flex-wrap items-center gap-2">
                <SessionKindBadge kind={m.session.kind} />
                <SessionStatusBadge status={m.session.status} />
                <AgentBadge agent={m.session.agent} />
                <span className="min-w-0 flex-1 truncate text-sm text-neutral-200">
                  {m.taskTitle ?? `${m.session.kind} session`}
                </span>
                <span className="text-[11px] text-neutral-500">{projectName(m.session.projectId)}</span>
                <span className="text-[11px] text-neutral-600">{formatRelative(m.session.createdAt)}</span>
              </div>
              <div className="text-xs text-neutral-400">
                <span className="mr-2 rounded bg-[var(--color-surface)] px-1.5 py-0.5 text-[10px] uppercase tracking-wide text-neutral-500">
                  {MATCH_LABELS[m.matchedIn] ?? m.matchedIn}
                </span>
                {m.snippet}
              </div>
            </Link>
          ))}
        </div>
      )}
    </div>
  );
}
