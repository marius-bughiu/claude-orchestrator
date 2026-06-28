import { useEffect, useState } from "react";
import { Save, AlertTriangle } from "lucide-react";
import { useStore } from "../store";
import * as api from "../api";
import type { AgentKind, PermissionMode, Settings, WebhookConfig } from "../api/types";
import { AGENT_LABELS } from "../lib/format";

function newWebhook(): WebhookConfig {
  const id =
    typeof crypto !== "undefined" && "randomUUID" in crypto
      ? crypto.randomUUID()
      : `wh-${Math.floor(Math.random() * 1e9)}`;
  return { id, name: "New webhook", url: "", kind: "slack", enabled: true, onTaskComplete: true, onTaskFail: true };
}

const PERMISSION_MODES: { value: PermissionMode; label: string; hint: string }[] = [
  { value: "bypass-permissions", label: "Bypass (autonomous)", hint: "Required for unattended runs. Skips all permission prompts." },
  { value: "accept-edits", label: "Accept edits", hint: "Auto-accepts file edits, prompts for other actions." },
  { value: "plan", label: "Plan only", hint: "Agent plans but does not execute." },
  { value: "default", label: "Default", hint: "Standard prompting (not suitable for autonomy)." },
];

const AGENTS: AgentKind[] = ["claude", "gemini", "codex"];

export function SettingsView() {
  const storeSettings = useStore((s) => s.settings);
  const refreshSettings = useStore((s) => s.refreshSettings);
  const refreshStatus = useStore((s) => s.refreshStatus);
  const [draft, setDraft] = useState<Settings | null>(storeSettings);
  const [saved, setSaved] = useState(false);

  useEffect(() => {
    if (!storeSettings) refreshSettings();
    else setDraft(storeSettings);
  }, [storeSettings, refreshSettings]);

  if (!draft) return <div className="p-6 text-sm text-neutral-500">Loading settings…</div>;

  const set = (patch: Partial<Settings>) => setDraft({ ...draft, ...patch });
  const setAgent = (kind: AgentKind, patch: Partial<Settings["agents"][string]>) => {
    const agents = { ...draft.agents, [kind]: { ...agentCfg(kind), ...patch } };
    setDraft({ ...draft, agents });
  };
  const agentCfg = (kind: AgentKind) =>
    draft.agents[kind] ?? {
      binary: null, model: null, extraArgs: [],
      limits: { sessionCostUsd: null, weeklyCostUsd: null, sessionTokenLimit: null, weeklyTokenLimit: null },
      sessionWindowHours: 5, weeklyWindowHours: 168, enabled: true,
    };

  const save = async () => {
    await api.updateSettings(draft);
    await refreshSettings();
    await refreshStatus();
    setSaved(true);
    setTimeout(() => setSaved(false), 1500);
  };

  const permHint = PERMISSION_MODES.find((m) => m.value === draft.permissionMode)?.hint;

  return (
    <div className="max-w-3xl p-6">
      <div className="mb-5 flex items-center justify-between">
        <div>
          <h1 className="text-lg font-semibold text-neutral-100">Settings</h1>
          <p className="text-xs text-neutral-500">Global orchestration behavior and per-agent configuration.</p>
        </div>
        <div className="flex items-center gap-2">
          {saved && <span className="text-xs text-emerald-400">Saved</span>}
          <button className="btn btn-primary" onClick={save}><Save size={14} /> Save</button>
        </div>
      </div>

      <section className="card mb-5 p-4">
        <h3 className="mb-3 text-sm font-semibold text-neutral-200">Scheduler</h3>
        <div className="grid grid-cols-2 gap-4">
          <label className="text-sm text-neutral-300">
            <span className="mb-1 block text-xs text-neutral-400">Max concurrent sessions</span>
            <input type="number" min={1} className="input" value={draft.maxConcurrent}
              onChange={(e) => set({ maxConcurrent: Math.max(1, Number(e.target.value)) })} />
          </label>
          <label className="text-sm text-neutral-300">
            <span className="mb-1 block text-xs text-neutral-400">Scheduler interval (seconds)</span>
            <input type="number" min={1} className="input" value={draft.tickIntervalSecs}
              onChange={(e) => set({ tickIntervalSecs: Math.max(1, Number(e.target.value)) })} />
          </label>
          <label className="text-sm text-neutral-300">
            <span className="mb-1 block text-xs text-neutral-400">Session timeout (seconds, 0 = none)</span>
            <input type="number" min={0} className="input" value={draft.sessionTimeoutSecs}
              onChange={(e) => set({ sessionTimeoutSecs: Math.max(0, Number(e.target.value)) })} />
          </label>
          <label className="text-sm text-neutral-300">
            <span className="mb-1 block text-xs text-neutral-400">Default agent</span>
            <select className="input" value={draft.defaultAgent} onChange={(e) => set({ defaultAgent: e.target.value as AgentKind })}>
              {AGENTS.map((a) => <option key={a} value={a}>{a}</option>)}
            </select>
          </label>
          <label className="text-sm text-neutral-300">
            <span className="mb-1 block text-xs text-neutral-400">Scheduled-task refresh (seconds)</span>
            <input type="number" min={30} className="input" value={draft.scheduleRefreshSecs}
              onChange={(e) => set({ scheduleRefreshSecs: Math.max(30, Number(e.target.value)) })} />
          </label>
        </div>
        <div className="mt-4 flex flex-wrap gap-6">
          <label className="flex items-center gap-2 text-sm text-neutral-300">
            <input type="checkbox" checked={draft.roadmapEnabled} onChange={(e) => set({ roadmapEnabled: e.target.checked })} />
            Roadmap loop (global)
          </label>
          <label className="flex items-center gap-2 text-sm text-neutral-300">
            <input type="checkbox" checked={draft.verifyEnabled} onChange={(e) => set({ verifyEnabled: e.target.checked })} />
            Auto-verify (global)
          </label>
          <label className="flex items-center gap-2 text-sm text-neutral-300">
            <input type="checkbox" checked={draft.balanceAgents} onChange={(e) => set({ balanceAgents: e.target.checked })} />
            Balance agent usage
          </label>
          <label className="flex items-center gap-2 text-sm text-neutral-300">
            <input type="checkbox" checked={draft.liveStreaming} onChange={(e) => set({ liveStreaming: e.target.checked })} />
            Live streaming &amp; injection
          </label>
          <label className="flex items-center gap-2 text-sm text-neutral-300">
            <input type="checkbox" checked={draft.notificationsEnabled} onChange={(e) => set({ notificationsEnabled: e.target.checked })} />
            Desktop notifications
          </label>
        </div>
      </section>

      <section className="card mb-5 p-4">
        <h3 className="mb-1 text-sm font-semibold text-neutral-200">Git isolation</h3>
        <p className="mb-3 text-xs text-neutral-500">Run each task in its own git worktree on a dedicated branch, so parallel agents never collide in the working tree.</p>
        <div className="flex flex-col gap-3">
          <label className="flex items-center gap-2 text-sm text-neutral-300">
            <input type="checkbox" checked={draft.isolateWorktrees} onChange={(e) => set({ isolateWorktrees: e.target.checked })} />
            Isolate tasks in per-task worktrees
          </label>
          <label className="flex items-center gap-2 text-sm text-neutral-300">
            <input type="checkbox" checked={draft.autoCommit} onChange={(e) => set({ autoCommit: e.target.checked })} disabled={!draft.isolateWorktrees} />
            Auto-commit task changes to the branch
          </label>
          <label className="flex items-center gap-2 text-sm text-neutral-300">
            <input type="checkbox" checked={draft.autoPr} onChange={(e) => set({ autoPr: e.target.checked })} disabled={!draft.isolateWorktrees || !draft.autoCommit} />
            Open a pull request when a task completes (needs <code className="text-neutral-400">gh</code>)
          </label>
        </div>
      </section>

      <section className="card mb-5 p-4">
        <div className="mb-1 flex items-center justify-between">
          <h3 className="text-sm font-semibold text-neutral-200">Notification webhooks</h3>
          <button
            className="btn !py-1"
            onClick={() => set({ webhooks: [...(draft.webhooks ?? []), newWebhook()] })}
          >
            + Add webhook
          </button>
        </div>
        <p className="mb-3 text-xs text-neutral-500">
          Post to Slack, Discord, or any endpoint when tasks finish. Delivered via <code className="text-neutral-400">curl</code>.
        </p>
        {(draft.webhooks ?? []).length === 0 ? (
          <p className="text-xs text-neutral-600">No webhooks configured.</p>
        ) : (
          <div className="flex flex-col gap-3">
            {(draft.webhooks ?? []).map((w, i) => {
              const update = (patch: Partial<WebhookConfig>) => {
                const webhooks = draft.webhooks.map((x, j) => (j === i ? { ...x, ...patch } : x));
                set({ webhooks });
              };
              const removeHook = () => set({ webhooks: draft.webhooks.filter((_, j) => j !== i) });
              return (
                <div key={w.id} className="rounded-md border border-[var(--color-border)] p-3">
                  <div className="mb-2 flex items-center gap-2">
                    <input
                      className="input max-w-[180px]"
                      value={w.name}
                      placeholder="Name"
                      onChange={(e) => update({ name: e.target.value })}
                    />
                    <select className="input max-w-[120px]" value={w.kind} onChange={(e) => update({ kind: e.target.value })}>
                      <option value="slack">Slack</option>
                      <option value="discord">Discord</option>
                      <option value="generic">Generic JSON</option>
                    </select>
                    <label className="ml-auto flex items-center gap-1.5 text-xs text-neutral-400">
                      <input type="checkbox" checked={w.enabled} onChange={(e) => update({ enabled: e.target.checked })} />
                      Enabled
                    </label>
                    <button className="btn btn-danger !px-2 !py-1" onClick={removeHook} title="Remove">
                      <AlertTriangle size={13} />
                    </button>
                  </div>
                  <input
                    className="input mb-2 font-mono text-xs"
                    value={w.url}
                    placeholder="https://hooks.slack.com/services/…"
                    onChange={(e) => update({ url: e.target.value })}
                  />
                  <div className="flex gap-4 text-xs text-neutral-400">
                    <label className="flex items-center gap-1.5">
                      <input type="checkbox" checked={w.onTaskComplete} onChange={(e) => update({ onTaskComplete: e.target.checked })} />
                      On task complete
                    </label>
                    <label className="flex items-center gap-1.5">
                      <input type="checkbox" checked={w.onTaskFail} onChange={(e) => update({ onTaskFail: e.target.checked })} />
                      On task fail
                    </label>
                  </div>
                </div>
              );
            })}
          </div>
        )}
      </section>

      <section className="card mb-5 p-4">
        <h3 className="mb-1 text-sm font-semibold text-neutral-200">Permissions</h3>
        <p className="mb-3 text-xs text-neutral-500">How much autonomy spawned agents have.</p>
        <select className="input max-w-sm" value={draft.permissionMode} onChange={(e) => set({ permissionMode: e.target.value as PermissionMode })}>
          {PERMISSION_MODES.map((m) => <option key={m.value} value={m.value}>{m.label}</option>)}
        </select>
        <div className="mt-2 flex items-start gap-2 text-xs text-amber-300/80">
          {draft.permissionMode === "bypass-permissions" && <AlertTriangle size={14} className="mt-0.5 shrink-0" />}
          <span className="text-neutral-500">{permHint}</span>
        </div>
      </section>

      <section className="card p-4">
        <h3 className="mb-3 text-sm font-semibold text-neutral-200">Agents</h3>
        <div className="flex flex-col gap-4">
          {AGENTS.map((kind) => {
            const cfg = agentCfg(kind);
            return (
              <div key={kind} className="rounded-md border border-[var(--color-border)] p-3">
                <div className="mb-2 flex items-center justify-between">
                  <span className="text-sm font-medium text-neutral-100">{AGENT_LABELS[kind]}</span>
                  <label className="flex items-center gap-2 text-xs text-neutral-400">
                    <input type="checkbox" checked={cfg.enabled} onChange={(e) => setAgent(kind, { enabled: e.target.checked })} />
                    enabled
                  </label>
                </div>
                <div className="grid grid-cols-2 gap-3 md:grid-cols-3">
                  <label className="text-xs text-neutral-400">
                    Binary
                    <input className="input mt-1" placeholder={kind} value={cfg.binary ?? ""}
                      onChange={(e) => setAgent(kind, { binary: e.target.value || null })} />
                  </label>
                  <label className="text-xs text-neutral-400">
                    Model
                    <input className="input mt-1" placeholder="default" value={cfg.model ?? ""}
                      onChange={(e) => setAgent(kind, { model: e.target.value || null })} />
                  </label>
                  <div />
                  <label className="text-xs text-neutral-400">
                    Session limit ($)
                    <input type="number" min={0} step="0.5" className="input mt-1" placeholder="none"
                      value={cfg.limits.sessionCostUsd ?? ""}
                      onChange={(e) => setAgent(kind, { limits: { ...cfg.limits, sessionCostUsd: e.target.value === "" ? null : Number(e.target.value) } })} />
                  </label>
                  <label className="text-xs text-neutral-400">
                    Session window (h)
                    <input type="number" min={1} className="input mt-1" value={cfg.sessionWindowHours}
                      onChange={(e) => setAgent(kind, { sessionWindowHours: Math.max(1, Number(e.target.value)) })} />
                  </label>
                  <div />
                  <label className="text-xs text-neutral-400">
                    Weekly limit ($)
                    <input type="number" min={0} step="1" className="input mt-1" placeholder="none"
                      value={cfg.limits.weeklyCostUsd ?? ""}
                      onChange={(e) => setAgent(kind, { limits: { ...cfg.limits, weeklyCostUsd: e.target.value === "" ? null : Number(e.target.value) } })} />
                  </label>
                  <label className="text-xs text-neutral-400">
                    Weekly window (h)
                    <input type="number" min={1} className="input mt-1" value={cfg.weeklyWindowHours}
                      onChange={(e) => setAgent(kind, { weeklyWindowHours: Math.max(1, Number(e.target.value)) })} />
                  </label>
                </div>
              </div>
            );
          })}
        </div>
      </section>
    </div>
  );
}
