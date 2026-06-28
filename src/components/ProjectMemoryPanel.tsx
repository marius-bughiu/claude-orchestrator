import { useEffect, useState } from "react";
import { BookText, RefreshCw, Lightbulb } from "lucide-react";
import * as api from "../api";
import type { ProjectMemory } from "../api/types";

/// Shows a project's accumulated memory: the auto-generated context document
/// and the lessons learned from verifier feedback. Both are injected into every
/// task prompt for the project.
export function ProjectMemoryPanel({ projectId }: { projectId: string }) {
  const [memory, setMemory] = useState<ProjectMemory>({ context: null, lessons: null });
  const [busy, setBusy] = useState(false);

  const load = () => api.projectMemory(projectId).then(setMemory).catch(() => {});
  useEffect(() => {
    load();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [projectId]);

  const regenerate = async () => {
    setBusy(true);
    try {
      await api.generateProjectContext(projectId);
      await load();
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="card p-4">
      <div className="mb-3 flex items-center justify-between">
        <h3 className="flex items-center gap-2 text-sm font-semibold text-neutral-200">
          <BookText size={15} className="text-indigo-400" /> Project memory
        </h3>
        <button className="btn !py-1" onClick={regenerate} disabled={busy}>
          <RefreshCw size={13} className={busy ? "animate-spin" : ""} /> Regenerate context
        </button>
      </div>
      <p className="mb-3 text-xs text-neutral-500">
        Context and lessons are written to <code className="text-neutral-400">.orchestrator/</code> and
        prepended to every task prompt for this project.
      </p>

      <div className="grid gap-4 lg:grid-cols-2">
        <div>
          <div className="mb-1.5 text-xs font-medium uppercase tracking-wide text-neutral-500">context.md</div>
          {memory.context ? (
            <pre className="max-h-64 overflow-auto rounded-md border border-[var(--color-border)] bg-[var(--color-surface)] p-3 text-[11px] leading-relaxed whitespace-pre-wrap text-neutral-300">
              {memory.context}
            </pre>
          ) : (
            <div className="rounded-md border border-dashed border-[var(--color-border)] p-4 text-center text-xs text-neutral-600">
              No context yet — click “Regenerate context”.
            </div>
          )}
        </div>
        <div>
          <div className="mb-1.5 flex items-center gap-1 text-xs font-medium uppercase tracking-wide text-neutral-500">
            <Lightbulb size={12} /> lessons.md
          </div>
          {memory.lessons ? (
            <pre className="max-h-64 overflow-auto rounded-md border border-[var(--color-border)] bg-[var(--color-surface)] p-3 text-[11px] leading-relaxed whitespace-pre-wrap text-neutral-300">
              {memory.lessons}
            </pre>
          ) : (
            <div className="rounded-md border border-dashed border-[var(--color-border)] p-4 text-center text-xs text-neutral-600">
              No lessons yet — these accumulate from verifier feedback as tasks run.
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
