import { useEffect, useState } from "react";
import { FileDiff, ChevronRight, ChevronDown } from "lucide-react";
import * as api from "../api";
import type { SessionDiff } from "../api/types";

function statusColor(status: string): string {
  switch (status) {
    case "untracked":
    case "added":
      return "text-emerald-400";
    case "deleted":
      return "text-rose-400";
    default:
      return "text-neutral-400";
  }
}

function DiffLine({ line }: { line: string }) {
  let cls = "text-neutral-400";
  if (line.startsWith("+") && !line.startsWith("+++")) cls = "text-emerald-300 bg-emerald-500/5";
  else if (line.startsWith("-") && !line.startsWith("---")) cls = "text-rose-300 bg-rose-500/5";
  else if (line.startsWith("@@")) cls = "text-indigo-300";
  else if (line.startsWith("diff ") || line.startsWith("index ") || line.startsWith("+++") || line.startsWith("---"))
    cls = "text-neutral-600";
  return <div className={`whitespace-pre ${cls}`}>{line || " "}</div>;
}

/// On-demand "Changes" panel: shows the git diff a task session produced on its
/// worktree branch. Collapsed by default; only fetches when opened.
export function SessionDiffPanel({ sessionId, hasBranch }: { sessionId: string; hasBranch: boolean }) {
  const [open, setOpen] = useState(false);
  const [diff, setDiff] = useState<SessionDiff | null>(null);
  const [loading, setLoading] = useState(false);

  useEffect(() => {
    if (!open || diff) return;
    setLoading(true);
    api.sessionDiff(sessionId)
      .then(setDiff)
      .finally(() => setLoading(false));
  }, [open, sessionId, diff]);

  // Refetch when reopened after live updates would have changed the tree.
  const reload = () => {
    setDiff(null);
    setOpen(true);
  };

  if (!hasBranch) return null;

  return (
    <div className="border-b border-[var(--color-border)] bg-[var(--color-surface)]">
      <button
        onClick={() => setOpen((o) => !o)}
        className="flex w-full items-center gap-2 px-4 py-2 text-xs font-medium text-neutral-300 hover:text-neutral-100"
      >
        {open ? <ChevronDown size={14} /> : <ChevronRight size={14} />}
        <FileDiff size={14} className="text-indigo-400" />
        Changes
        {diff && diff.available && (
          <span className="ml-1 text-[11px] text-neutral-500">
            {diff.files.length} file{diff.files.length === 1 ? "" : "s"}
            <span className="text-emerald-400"> +{diff.additions}</span>
            <span className="text-rose-400"> −{diff.deletions}</span>
          </span>
        )}
        {open && (
          <span
            role="button"
            tabIndex={0}
            onClick={(e) => { e.stopPropagation(); reload(); }}
            onKeyDown={(e) => { if (e.key === "Enter") { e.stopPropagation(); reload(); } }}
            className="ml-auto cursor-pointer text-[11px] text-indigo-300 hover:underline"
          >
            refresh
          </span>
        )}
      </button>

      {open && (
        <div className="px-4 pb-3">
          {loading && <p className="py-2 text-xs text-neutral-500">Loading diff…</p>}
          {!loading && diff && !diff.available && (
            <p className="py-2 text-xs text-neutral-500">
              No changes recorded for this session (not isolated, no commit, or the branch was cleaned up).
            </p>
          )}
          {!loading && diff && diff.available && (
            <>
              <div className="mb-2 flex flex-col gap-0.5">
                {diff.files.map((f) => (
                  <div key={f.path} className="flex items-center gap-2 text-xs">
                    <span className={`font-mono ${statusColor(f.status)}`}>{f.path}</span>
                    {f.status === "untracked" ? (
                      <span className="text-[10px] uppercase text-emerald-500/70">new</span>
                    ) : (
                      <span className="text-[11px] text-neutral-600">
                        <span className="text-emerald-400">+{f.additions}</span>{" "}
                        <span className="text-rose-400">−{f.deletions}</span>
                      </span>
                    )}
                  </div>
                ))}
              </div>
              {diff.base && (
                <div className="mb-1 text-[11px] text-neutral-600">
                  {diff.branch} vs {diff.base}
                </div>
              )}
              {diff.patch && (
                <pre className="max-h-96 overflow-auto rounded-md border border-[var(--color-border)] bg-[var(--color-bg)] p-2 font-mono text-[11px] leading-relaxed">
                  {diff.patch.split("\n").map((line, i) => (
                    <DiffLine key={i} line={line} />
                  ))}
                </pre>
              )}
              {diff.truncated && (
                <p className="mt-1 text-[11px] text-amber-400/80">Diff truncated — open the branch locally for the full change.</p>
              )}
            </>
          )}
        </div>
      )}
    </div>
  );
}
