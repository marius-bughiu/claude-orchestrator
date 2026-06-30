import { useEffect, useRef, useState } from "react";
import { useParams, useNavigate, Link } from "react-router-dom";
import { ArrowLeft, Square, Send, Wrench, Brain, CheckCircle2, AlertTriangle, Terminal, User, GitBranch, GitPullRequest, Download, FileText } from "lucide-react";
import * as api from "../api";
import type { Session, SessionEvent } from "../api/types";
import { useStore } from "../store";
import { SessionKindBadge, SessionStatusBadge, AgentBadge } from "../components/Badges";
import { ModelInput } from "../components/ModelInput";
import { SessionDiffPanel } from "../components/SessionDiffPanel";
import { formatCost, formatDuration, formatTokens } from "../lib/format";

/** Strip an agent's absolute path down to a repo-relative one for display. */
function relativePath(p: string, root?: string): string {
  if (!p) return p;
  if (root && p.startsWith(root)) return p.slice(root.length).replace(/^\/+/, "");
  // Isolated runs live in a temp worktree: …/claude-orchestrator-worktrees/<id>/<rel>.
  const m = p.match(/claude-orchestrator-worktrees\/[^/]+\/(.*)$/);
  if (m) return m[1];
  return p.replace(/^\/(?:Users|home)\/[^/]+\//, "~/");
}

const str = (v: unknown): string | undefined => (typeof v === "string" ? v : undefined);

/** The single most useful argument to show next to a tool name — no JSON dump. */
function toolArg(name: string, input: Record<string, unknown>, root?: string): string | null {
  const fp = str(input.file_path) ?? str(input.path) ?? str(input.notebook_path);
  switch (name) {
    case "Read":
    case "Edit":
    case "MultiEdit":
    case "Write":
    case "NotebookEdit":
      return fp ? relativePath(fp, root) : null;
    case "Bash":
      return str(input.command) ?? null;
    case "Grep":
    case "Glob":
      return str(input.pattern) ?? null;
    case "Task":
      return str(input.description) ?? null;
    case "WebFetch":
    case "WebSearch":
      return str(input.url) ?? str(input.query) ?? null;
    case "TodoWrite":
      return null;
    default:
      return fp ? relativePath(fp, root) : (str(input.pattern) ?? str(input.command) ?? str(input.url) ?? null);
  }
}

const toolName = (e: SessionEvent): string => ((e.data as { name?: string } | null)?.name) ?? e.text ?? "";
const toolInput = (e: SessionEvent): Record<string, unknown> => ((e.data as { input?: Record<string, unknown> } | null)?.input) ?? {};
const isErrorResult = (e: SessionEvent): boolean => Boolean((e.data as { isError?: boolean } | null)?.isError);

type RenderItem = { kind: "event"; event: SessionEvent } | { kind: "reads"; id: number; paths: string[] };

/** Collapse the raw event stream for display: drop redundant (successful) tool
 *  output, and group runs of consecutive Reads into a single row. */
function buildRenderItems(events: SessionEvent[], root?: string): RenderItem[] {
  // Successful tool results just echo file contents / command output — noise in
  // this timeline. Keep only errors, which are worth surfacing.
  const visible = events.filter((e) => !(e.kind === "tool_result" && !isErrorResult(e)));
  const items: RenderItem[] = [];
  let i = 0;
  while (i < visible.length) {
    const e = visible[i];
    if (e.kind === "tool_use" && toolName(e) === "Read") {
      const paths: string[] = [];
      const id = e.id;
      while (i < visible.length && visible[i].kind === "tool_use" && toolName(visible[i]) === "Read") {
        const fp = str(toolInput(visible[i]).file_path);
        paths.push(fp ? relativePath(fp, root) : "(unknown)");
        i++;
      }
      items.push({ kind: "reads", id, paths });
    } else {
      items.push({ kind: "event", event: e });
      i++;
    }
  }
  return items;
}

function ReadsRow({ paths }: { paths: string[] }) {
  return (
    <div className="flex gap-2 px-3 py-1 text-xs text-amber-300/90">
      <FileText size={13} className="mt-0.5 shrink-0" />
      {paths.length === 1 ? (
        <span className="min-w-0 truncate">
          <span className="font-medium">Read</span> <span className="text-neutral-400">{paths[0]}</span>
        </span>
      ) : (
        <div className="min-w-0">
          <span className="font-medium">Read {paths.length} files</span>
          <div className="mt-0.5 flex flex-col gap-0.5 text-[11px] text-neutral-400">
            {paths.map((p, idx) => <span key={idx} className="truncate">{p}</span>)}
          </div>
        </div>
      )}
    </div>
  );
}

function EventRow({ event, root }: { event: SessionEvent; root?: string }) {
  const data = event.data as Record<string, unknown> | null;
  switch (event.kind) {
    case "user_message":
      return (
        <div className="flex justify-end">
          <div className="flex max-w-[80%] items-start gap-2 rounded-md border border-indigo-500/30 bg-indigo-600/10 px-3 py-2 text-sm text-indigo-100">
            <User size={13} className="mt-0.5 shrink-0 text-indigo-300" />
            <span className="whitespace-pre-wrap">{event.text}</span>
          </div>
        </div>
      );
    case "assistant":
      return (
        <div className="whitespace-pre-wrap rounded-md bg-[var(--color-surface-2)] px-3 py-2 text-sm text-neutral-200">
          {event.text}
        </div>
      );
    case "thinking":
      return (
        <div className="flex gap-2 px-3 py-1 text-xs italic text-neutral-500">
          <Brain size={13} className="mt-0.5 shrink-0" />
          <span className="whitespace-pre-wrap">{event.text}</span>
        </div>
      );
    case "tool_use": {
      const name = toolName(event);
      const arg = toolArg(name, toolInput(event), root);
      return (
        <div className="flex gap-2 px-3 py-1 text-xs text-amber-300/90">
          <Wrench size={13} className="mt-0.5 shrink-0" />
          <span className="min-w-0 truncate">
            <span className="font-medium">{name}</span>
            {arg && <span className="ml-1.5 text-neutral-400">{arg}</span>}
          </span>
        </div>
      );
    }
    case "tool_result":
      return (
        <div className="flex gap-2 px-3 py-1 text-[11px] text-neutral-500">
          <Terminal size={12} className="mt-0.5 shrink-0" />
          <span className="line-clamp-3 whitespace-pre-wrap">{event.text}</span>
        </div>
      );
    case "result": {
      const usage = (data?.usage ?? {}) as Record<string, number>;
      return (
        <div className="flex items-center gap-2 rounded-md border border-emerald-500/20 bg-emerald-500/5 px-3 py-2 text-xs text-emerald-300">
          <CheckCircle2 size={14} />
          <span>{event.text || "Completed"}</span>
          <span className="ml-auto text-neutral-500">
            {formatTokens((usage.inputTokens ?? 0) + (usage.outputTokens ?? 0))} tok ·{" "}
            {formatCost(usage.totalCostUsd ?? 0)}
          </span>
        </div>
      );
    }
    case "error":
      return (
        <div className="flex gap-2 rounded-md border border-red-500/30 bg-red-500/5 px-3 py-2 text-xs text-red-300">
          <AlertTriangle size={14} className="shrink-0" />
          <span className="whitespace-pre-wrap">{event.text}</span>
        </div>
      );
    case "init":
      return <div className="px-3 py-1 text-[11px] text-neutral-600">— {event.text} —</div>;
    default:
      return null;
  }
}

export function SessionDetailView() {
  const { id = "" } = useParams();
  const navigate = useNavigate();
  const [session, setSession] = useState<Session | null>(null);
  const [events, setEvents] = useState<SessionEvent[]>([]);
  const [streaming, setStreaming] = useState<{ assistant: string; thinking: string }>({ assistant: "", thinking: "" });
  const [message, setMessage] = useState("");
  const [model, setModel] = useState("");
  const [sending, setSending] = useState(false);
  const projects = useStore((s) => s.projects);
  const root = session ? projects.find((p) => p.id === session.projectId)?.path : undefined;
  const scrollRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    let active = true;
    setSession(null);
    setEvents([]);
    setStreaming({ assistant: "", thinking: "" });
    api.getSession(id).then((s) => active && setSession(s)).catch(() => {});
    api.getSessionEvents(id).then((e) => active && setEvents(e)).catch(() => {});
    const unlisten = api.onOrchestratorEvent((e) => {
      if (e.type === "sessionEvent" && e.sessionId === id) {
        // A settled assistant/thinking message clears the matching live buffer.
        setEvents((prev) => [...prev, e.event]);
        if (e.event.kind === "assistant") setStreaming((s) => ({ ...s, assistant: "" }));
        if (e.event.kind === "thinking") setStreaming((s) => ({ ...s, thinking: "" }));
      } else if (e.type === "sessionDelta" && e.sessionId === id) {
        setStreaming((s) =>
          e.kind === "thinking"
            ? { ...s, thinking: s.thinking + e.text }
            : { ...s, assistant: s.assistant + e.text },
        );
      } else if (e.type === "sessionUpdated" && e.session.id === id) {
        setSession(e.session);
        if (e.session.status !== "running" && e.session.status !== "pending") {
          setStreaming({ assistant: "", thinking: "" });
        }
      }
    });
    return () => {
      active = false;
      unlisten.then((u) => u());
    };
  }, [id]);

  useEffect(() => {
    scrollRef.current?.scrollTo({ top: scrollRef.current.scrollHeight, behavior: "smooth" });
  }, [events.length, streaming.assistant, streaming.thinking]);

  const stop = async () => {
    try {
      await api.stopSession(id);
    } catch (e) {
      console.error(e);
    }
  };

  const send = async () => {
    if (!message.trim()) return;
    setSending(true);
    try {
      // Live sessions inject in place (same id); finished ones resume (new id).
      const targetId = await api.injectMessage(id, message.trim(), model.trim() || undefined);
      setMessage("");
      if (targetId !== id) navigate(`/sessions/${targetId}`);
    } catch (e) {
      console.error(e);
    } finally {
      setSending(false);
    }
  };

  if (!session) return <div className="p-6 text-sm text-neutral-500">Loading session…</div>;

  const isActive = session.status === "running" || session.status === "pending";

  const exportTranscript = () => {
    const s = session;
    const lines: string[] = [];
    lines.push(`# Session transcript`);
    lines.push("");
    lines.push(`- **Session:** ${s.id}`);
    lines.push(`- **Agent:** ${s.agent}${s.model ? ` (${s.model})` : ""}`);
    lines.push(`- **Kind:** ${s.kind}`);
    lines.push(`- **Status:** ${s.status}`);
    if (s.branch) lines.push(`- **Branch:** ${s.branch}`);
    if (s.prUrl) lines.push(`- **Pull request:** ${s.prUrl}`);
    lines.push(`- **Started:** ${s.startedAt ?? "—"}`);
    lines.push(`- **Tokens:** ${s.usage.inputTokens + s.usage.outputTokens} · **Cost:** ${formatCost(s.usage.totalCostUsd)} · **Turns:** ${s.usage.numTurns}`);
    lines.push("");
    lines.push(`## Prompt`);
    lines.push("");
    lines.push(s.prompt);
    lines.push("");
    lines.push(`## Transcript`);
    lines.push("");
    for (const e of events) {
      const label = e.kind.replace(/_/g, " ");
      lines.push(`### ${label}`);
      if (e.text) {
        lines.push("");
        lines.push(e.kind === "tool_use" ? "```\n" + e.text + "\n```" : e.text);
      }
      lines.push("");
    }
    const blob = new Blob([lines.join("\n")], { type: "text/markdown" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = `session-${s.id.slice(0, 8)}.md`;
    a.click();
    URL.revokeObjectURL(url);
  };

  return (
    <div className="flex h-full flex-col">
      <div className="border-b border-[var(--color-border)] p-4">
        <button className="mb-3 flex items-center gap-1 text-xs text-neutral-400 hover:text-neutral-200" onClick={() => navigate(-1)}>
          <ArrowLeft size={14} /> Back
        </button>
        <div className="flex items-center gap-2">
          <SessionKindBadge kind={session.kind} />
          <SessionStatusBadge status={session.status} />
          <AgentBadge agent={session.agent} />
          {session.model && <span className="text-xs text-neutral-500">{session.model}</span>}
          {session.branch && (
            <span className="inline-flex items-center gap-1 rounded bg-[var(--color-surface)] px-1.5 py-0.5 font-mono text-[11px] text-neutral-400" title="Isolated worktree branch">
              <GitBranch size={11} /> {session.branch}
            </span>
          )}
          {session.prUrl && (
            <a href={session.prUrl} target="_blank" rel="noreferrer" className="inline-flex items-center gap-1 rounded bg-emerald-500/10 px-1.5 py-0.5 text-[11px] text-emerald-300 hover:underline" title="Pull request">
              <GitPullRequest size={11} /> PR
            </a>
          )}
          <button
            onClick={exportTranscript}
            className="ml-auto inline-flex items-center gap-1 text-xs text-neutral-400 hover:text-neutral-200"
            title="Export transcript as Markdown"
          >
            <Download size={13} /> Export
          </button>
          <Link to={`/projects/${session.projectId}`} className="text-xs text-indigo-300 hover:underline">
            view project
          </Link>
        </div>
        <div className="mt-2 flex items-center gap-4 text-xs text-neutral-500">
          <span>Duration: {formatDuration(session.startedAt, session.endedAt)}</span>
          <span>Turns: {session.usage.numTurns}</span>
          <span>
            Tokens: {formatTokens(session.usage.inputTokens + session.usage.outputTokens)}
          </span>
          <span>Cost: {formatCost(session.usage.totalCostUsd)}</span>
          {isActive && (
            <button className="btn btn-danger ml-auto !py-1" onClick={stop}>
              <Square size={12} /> Stop
            </button>
          )}
        </div>
      </div>

      <SessionDiffPanel sessionId={session.id} hasBranch={!!session.branch} />

      <div ref={scrollRef} className="flex min-h-0 flex-1 flex-col gap-1.5 overflow-y-auto p-4">
        <div className="rounded-md border border-[var(--color-border)] bg-[var(--color-surface)] px-3 py-2 text-xs text-neutral-400">
          <span className="text-neutral-500">Prompt:</span>{" "}
          <span className="whitespace-pre-wrap">{session.prompt.slice(0, 600)}{session.prompt.length > 600 ? "…" : ""}</span>
        </div>
        {buildRenderItems(events, root).map((item) =>
          item.kind === "reads"
            ? <ReadsRow key={`reads-${item.id}`} paths={item.paths} />
            : <EventRow key={item.event.id} event={item.event} root={root} />
        )}
        {streaming.thinking && (
          <div className="flex gap-2 px-3 py-1 text-xs italic text-neutral-500">
            <Brain size={13} className="mt-0.5 shrink-0" />
            <span className="whitespace-pre-wrap">{streaming.thinking}</span>
          </div>
        )}
        {streaming.assistant && (
          <div className="whitespace-pre-wrap rounded-md bg-[var(--color-surface-2)] px-3 py-2 text-sm text-neutral-200">
            {streaming.assistant}
            <span className="ml-0.5 inline-block h-3.5 w-1.5 animate-pulse bg-indigo-400 align-middle" />
          </div>
        )}
        {events.length === 0 && !streaming.assistant && !streaming.thinking && (
          <div className="py-8 text-center text-sm text-neutral-600">No events recorded.</div>
        )}
      </div>

      <div className="border-t border-[var(--color-border)] p-3">
        <div className="flex gap-2">
          <input
            className="input"
            placeholder={isActive ? "Inject a message into this running session…" : "Send a follow-up (resumes in a new session)…"}
            value={message}
            onChange={(e) => setMessage(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && !e.shiftKey && send()}
          />
          <div className="w-40 shrink-0">
            <ModelInput agent={session.agent} value={model} onChange={setModel} id="session-model" />
          </div>
          <button className="btn btn-primary shrink-0" onClick={send} disabled={sending || !message.trim()}>
            <Send size={14} /> Send
          </button>
        </div>
      </div>
    </div>
  );
}
