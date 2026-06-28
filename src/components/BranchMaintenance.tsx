import { useCallback, useEffect, useState } from "react";
import { GitBranch, Trash2, RefreshCw, Eraser } from "lucide-react";
import * as api from "../api";
import type { BranchInfo } from "../api/types";

/// Lists the orchestrator-created branches in a project repo so stale ones (from
/// committed-but-unmerged tasks) can be cleaned up, plus a worktree prune.
export function BranchMaintenance({ projectId }: { projectId: string }) {
  const [branches, setBranches] = useState<BranchInfo[] | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(() => {
    setError(null);
    api.listBranches(projectId).then(setBranches).catch((e) => { setBranches([]); setError(String(e)); });
  }, [projectId]);

  useEffect(() => { load(); }, [load]);

  const del = async (name: string) => {
    setBusy(name);
    try {
      await api.deleteBranch(projectId, name);
      load();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(null);
    }
  };

  const prune = async () => {
    setBusy("__prune__");
    try {
      await api.pruneWorktrees(projectId);
      load();
    } finally {
      setBusy(null);
    }
  };

  // Nothing to show and no error: keep the panel out of the way.
  if (branches && branches.length === 0 && !error) return null;

  return (
    <div className="card p-4">
      <div className="mb-2 flex items-center justify-between">
        <h3 className="flex items-center gap-2 text-sm font-semibold text-neutral-200">
          <GitBranch size={15} className="text-indigo-400" /> Task branches
        </h3>
        <div className="flex gap-2">
          <button className="btn !py-1" onClick={prune} disabled={busy === "__prune__"} title="Prune stale worktree metadata">
            <Eraser size={13} /> Prune worktrees
          </button>
          <button className="btn !py-1" onClick={load} title="Refresh">
            <RefreshCw size={13} />
          </button>
        </div>
      </div>
      <p className="mb-3 text-xs text-neutral-500">
        Local <code className="text-neutral-400">orchestrator/*</code> branches from isolated tasks. Merged branches are safe to delete.
      </p>
      {error && <p className="text-xs text-amber-400/80">{error}</p>}
      {branches && branches.length > 0 && (
        <div className="flex flex-col gap-1.5">
          {branches.map((b) => (
            <div key={b.name} className="flex items-center gap-2 rounded-md border border-[var(--color-border)] bg-[var(--color-surface)] px-3 py-1.5">
              <span className="truncate font-mono text-xs text-neutral-300">{b.name}</span>
              {b.merged && <span className="chip border border-emerald-500/30 text-emerald-400">merged</span>}
              {b.active && <span className="chip border border-sky-500/30 text-sky-400">in use</span>}
              <button
                className="btn btn-danger ml-auto !px-2 !py-1"
                disabled={b.active || busy === b.name}
                onClick={() => del(b.name)}
                title={b.active ? "A running session is using this branch" : "Delete branch"}
              >
                <Trash2 size={13} />
              </button>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
