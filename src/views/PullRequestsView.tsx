import { useEffect, useState, useCallback } from "react";
import { openPath } from "@tauri-apps/plugin-opener";
import { GitPullRequest, GitMerge, ExternalLink, RefreshCw, CheckCircle2, XCircle, Clock } from "lucide-react";
import * as api from "../api";
import { useStore } from "../store";
import type { Project, PullRequest } from "../api/types";
import { EmptyState } from "../components/Modal";

function CiBadge({ ci }: { ci: string }) {
  const map: Record<string, { icon: React.ReactNode; cls: string; label: string }> = {
    passing: { icon: <CheckCircle2 size={13} />, cls: "text-emerald-400", label: "CI passing" },
    failing: { icon: <XCircle size={13} />, cls: "text-rose-400", label: "CI failing" },
    pending: { icon: <Clock size={13} />, cls: "text-amber-400", label: "CI running" },
    none: { icon: null, cls: "text-neutral-600", label: "no checks" },
  };
  const c = map[ci] ?? map.none;
  return (
    <span className={`inline-flex items-center gap-1 text-xs ${c.cls}`}>
      {c.icon} {c.label}
    </span>
  );
}

function ReviewBadge({ decision }: { decision: string | null }) {
  if (!decision) return null;
  const map: Record<string, { cls: string; label: string }> = {
    APPROVED: { cls: "text-emerald-400", label: "approved" },
    CHANGES_REQUESTED: { cls: "text-rose-400", label: "changes requested" },
    REVIEW_REQUIRED: { cls: "text-neutral-500", label: "review required" },
  };
  const c = map[decision];
  if (!c) return null;
  return <span className={`text-xs ${c.cls}`}>{c.label}</span>;
}

function ProjectPRs({ project }: { project: Project }) {
  const [prs, setPrs] = useState<PullRequest[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [merging, setMerging] = useState<number | null>(null);

  const load = useCallback(() => {
    setError(null);
    api.listPullRequests(project.id)
      .then(setPrs)
      .catch((e) => { setPrs([]); setError(String(e)); });
  }, [project.id]);

  useEffect(() => { load(); }, [load]);

  const merge = async (number: number) => {
    setMerging(number);
    try {
      await api.mergePullRequest(project.id, number);
      load();
    } catch (e) {
      setError(String(e));
    } finally {
      setMerging(null);
    }
  };

  // Hide projects with no PRs and no error (keeps the page focused).
  if (prs && prs.length === 0 && !error) return null;

  return (
    <div className="card mb-4 p-4">
      <div className="mb-2 flex items-center justify-between">
        <h3 className="text-sm font-semibold text-neutral-200">{project.name}</h3>
        <button className="btn !py-1" onClick={load} title="Refresh">
          <RefreshCw size={13} />
        </button>
      </div>
      {error && <p className="text-xs text-amber-400/80">{error}</p>}
      {prs && prs.length > 0 && (
        <div className="flex flex-col gap-2">
          {prs.map((pr) => (
            <div key={pr.number} className="flex items-center gap-3 rounded-md border border-[var(--color-border)] bg-[var(--color-surface)] px-3 py-2">
              <GitPullRequest size={15} className={pr.draft ? "text-neutral-500" : "text-emerald-400"} />
              <div className="min-w-0 flex-1">
                <div className="flex items-center gap-2">
                  <span className="truncate text-sm text-neutral-200">{pr.title}</span>
                  <span className="text-xs text-neutral-600">#{pr.number}</span>
                  {pr.draft && <span className="chip border border-[var(--color-border)] text-neutral-500">draft</span>}
                </div>
                <div className="mt-0.5 flex items-center gap-3">
                  <span className="font-mono text-[11px] text-neutral-500">{pr.branch}</span>
                  <CiBadge ci={pr.ci} />
                  <ReviewBadge decision={pr.reviewDecision} />
                  {pr.mergeable === "CONFLICTING" && <span className="text-xs text-rose-400">conflicts</span>}
                </div>
              </div>
              <button className="btn !px-2 !py-1" onClick={() => openPath(pr.url)} title="Open on GitHub">
                <ExternalLink size={13} />
              </button>
              <button
                className="btn btn-primary !px-2 !py-1"
                disabled={merging === pr.number || pr.draft || pr.mergeable === "CONFLICTING"}
                onClick={() => merge(pr.number)}
                title="Squash & merge"
              >
                <GitMerge size={13} /> {merging === pr.number ? "Merging…" : "Merge"}
              </button>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

export function PullRequestsView() {
  const projects = useStore((s) => s.projects);
  const refreshProjects = useStore((s) => s.refreshProjects);

  useEffect(() => {
    if (projects.length === 0) refreshProjects();
  }, [projects.length, refreshProjects]);

  return (
    <div className="p-6">
      <div className="mb-5">
        <h1 className="text-lg font-semibold text-neutral-100">Pull requests</h1>
        <p className="text-xs text-neutral-500">Open PRs across your projects, with CI and review status. Requires the <code className="text-neutral-400">gh</code> CLI.</p>
      </div>
      {projects.length === 0 ? (
        <EmptyState title="No projects" hint="Add a project to track its pull requests." />
      ) : (
        projects.map((p) => <ProjectPRs key={p.id} project={p} />)
      )}
    </div>
  );
}
