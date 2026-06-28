import { useEffect, useRef, useState } from "react";
import { useParams, useNavigate, Link } from "react-router-dom";
import { ArrowLeft, Square, Send, Wrench, Brain, CheckCircle2, AlertTriangle, Terminal } from "lucide-react";
import * as api from "../api";
import type { Session, SessionEvent } from "../api/types";
import { SessionKindBadge, SessionStatusBadge, AgentBadge } from "../components/Badges";
import { formatCost, formatDuration, formatTokens } from "../lib/format";

function EventRow({ event }: { event: SessionEvent }) {
  const data = event.data as Record<string, unknown> | null;
  switch (event.kind) {
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
    case "tool_use":
      return (
        <div className="flex gap-2 px-3 py-1 text-xs text-amber-300/90">
          <Wrench size={13} className="mt-0.5 shrink-0" />
          <div className="min-w-0">
            <span className="font-medium">{event.text}</span>
            {data?.input != null && (
              <pre className="mt-0.5 max-h-40 overflow-auto rounded bg-black/30 p-1.5 text-[11px] text-neutral-400">
                {JSON.stringify(data.input, null, 2)}
              </pre>
            )}
          </div>
        </div>
      );
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
  const [message, setMessage] = useState("");
  const [sending, setSending] = useState(false);
  const scrollRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    let active = true;
    setSession(null);
    setEvents([]);
    api.getSession(id).then((s) => active && setSession(s)).catch(() => {});
    api.getSessionEvents(id).then((e) => active && setEvents(e)).catch(() => {});
    const unlisten = api.onOrchestratorEvent((e) => {
      if (e.type === "sessionEvent" && e.sessionId === id) {
        setEvents((prev) => [...prev, e.event]);
      } else if (e.type === "sessionUpdated" && e.session.id === id) {
        setSession(e.session);
      }
    });
    return () => {
      active = false;
      unlisten.then((u) => u());
    };
  }, [id]);

  useEffect(() => {
    scrollRef.current?.scrollTo({ top: scrollRef.current.scrollHeight, behavior: "smooth" });
  }, [events.length]);

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
      const newId = await api.sendMessage(id, message.trim());
      setMessage("");
      navigate(`/sessions/${newId}`);
    } catch (e) {
      console.error(e);
    } finally {
      setSending(false);
    }
  };

  if (!session) return <div className="p-6 text-sm text-neutral-500">Loading session…</div>;

  const isActive = session.status === "running" || session.status === "pending";

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
          <Link to={`/projects/${session.projectId}`} className="ml-auto text-xs text-indigo-300 hover:underline">
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

      <div ref={scrollRef} className="flex min-h-0 flex-1 flex-col gap-1.5 overflow-y-auto p-4">
        <div className="rounded-md border border-[var(--color-border)] bg-[var(--color-surface)] px-3 py-2 text-xs text-neutral-400">
          <span className="text-neutral-500">Prompt:</span>{" "}
          <span className="whitespace-pre-wrap">{session.prompt.slice(0, 600)}{session.prompt.length > 600 ? "…" : ""}</span>
        </div>
        {events.map((e) => <EventRow key={e.id} event={e} />)}
        {events.length === 0 && (
          <div className="py-8 text-center text-sm text-neutral-600">No events recorded.</div>
        )}
      </div>

      <div className="border-t border-[var(--color-border)] p-3">
        <div className="flex gap-2">
          <input
            className="input"
            placeholder="Send a follow-up message (continues this conversation in a new session)…"
            value={message}
            onChange={(e) => setMessage(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && !e.shiftKey && send()}
          />
          <button className="btn btn-primary shrink-0" onClick={send} disabled={sending || !message.trim()}>
            <Send size={14} /> Send
          </button>
        </div>
      </div>
    </div>
  );
}
