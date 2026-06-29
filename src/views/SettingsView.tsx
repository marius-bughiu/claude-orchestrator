import { useEffect, useRef, useState } from "react";
import { Save, AlertTriangle, RefreshCw, CheckCircle2, XCircle, Stethoscope } from "lucide-react";
import { useStore } from "../store";
import * as api from "../api";
import type { AgentKind, Diagnostic, PermissionMode, Settings, WebhookConfig } from "../api/types";
import { AGENT_LABELS } from "../lib/format";

function newWebhook(): WebhookConfig {
  const id =
    typeof crypto !== "undefined" && "randomUUID" in crypto
      ? crypto.randomUUID()
      : `wh-${Math.floor(Math.random() * 1e9)}`;
  return { id, name: "New webhook", url: "", kind: "slack", enabled: true, onTaskComplete: true, onTaskFail: true, projectIds: [], template: "" };
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
  const projects = useStore((s) => s.projects);
  const refreshSettings = useStore((s) => s.refreshSettings);
  const refreshStatus = useStore((s) => s.refreshStatus);
  const refreshProjects = useStore((s) => s.refreshProjects);
  const diagnosticsNonce = useStore((s) => s.diagnosticsNonce);
  const [draft, setDraft] = useState<Settings | null>(storeSettings);
  const [saved, setSaved] = useState(false);
  const [health, setHealth] = useState<Record<string, { available: boolean; version: string | null }>>({});
  const [diags, setDiags] = useState<Diagnostic[] | null>(null);
  const [diagBusy, setDiagBusy] = useState(false);
  const [tests, setTests] = useState<Record<string, { ok: boolean; msg: string } | "sending">>({});
  const [configNotice, setConfigNotice] = useState<{ ok: boolean; msg: string } | null>(null);
  const [backupNotice, setBackupNotice] = useState<{ ok: boolean; msg: string } | null>(null);
  const importFileRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (!storeSettings) refreshSettings();
    else setDraft(storeSettings);
  }, [storeSettings, refreshSettings]);

  const loadHealth = () =>
    api.agentHealth().then((hs) => {
      setHealth(Object.fromEntries(hs.map((h) => [h.agent, { available: h.available, version: h.version }])));
    }).catch(() => {});
  useEffect(() => { loadHealth(); }, []);

  const runDiagnostics = () => {
    setDiagBusy(true);
    api.diagnostics()
      .then(setDiags)
      .catch((e) => setDiags([{ category: "system", name: "diagnostics", level: "error", detail: String(e) }]))
      .finally(() => setDiagBusy(false));
  };

  // The command palette can request a diagnostics run after navigating here.
  useEffect(() => {
    if (diagnosticsNonce > 0) runDiagnostics();
  }, [diagnosticsNonce]); // eslint-disable-line react-hooks/exhaustive-deps

  const exportConfig = async () => {
    try {
      const bundle = await api.exportConfig();
      const url = URL.createObjectURL(new Blob([JSON.stringify(bundle, null, 2)], { type: "application/json" }));
      const a = document.createElement("a");
      a.href = url;
      a.download = "orchestrator-config.json";
      a.click();
      URL.revokeObjectURL(url);
      setConfigNotice({ ok: true, msg: "Exported." });
    } catch (e) {
      setConfigNotice({ ok: false, msg: String(e) });
    }
  };
  const onImportFile = async (e: React.ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0];
    e.target.value = "";
    if (!file) return;
    try {
      const bundle = JSON.parse(await file.text());
      const res = await api.importConfig(bundle);
      await refreshSettings();
      await refreshProjects();
      await refreshStatus();
      setConfigNotice({ ok: true, msg: `Imported ${res.projectsImported} project(s)${res.projectsSkipped ? `, skipped ${res.projectsSkipped}` : ""}.` });
    } catch (err) {
      setConfigNotice({ ok: false, msg: `Import failed: ${err}` });
    }
  };

  const backupNow = async () => {
    setBackupNotice(null);
    try {
      const path = await api.backupConfigNow();
      setBackupNotice({ ok: true, msg: `Saved to ${path}` });
    } catch (e) {
      setBackupNotice({ ok: false, msg: String(e) });
    }
  };

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
        <h3 className="mb-1 text-sm font-semibold text-neutral-200">Retries</h3>
        <p className="mb-3 text-xs text-neutral-500">
          Failed tasks wait an exponentially growing backoff before retrying. Scheduled tasks are never retried — they run again on their own cadence.
        </p>
        <label className="mb-3 flex items-center gap-2 text-sm text-neutral-300">
          <input type="checkbox" checked={draft.retryEnabled} onChange={(e) => set({ retryEnabled: e.target.checked })} />
          Retry failed tasks with backoff
        </label>
        <div className="grid grid-cols-2 gap-4">
          <label className="text-sm text-neutral-300">
            <span className="mb-1 block text-xs text-neutral-400">Base backoff (seconds)</span>
            <input type="number" min={1} className="input" value={draft.retryBaseSecs} disabled={!draft.retryEnabled}
              onChange={(e) => set({ retryBaseSecs: Math.max(1, Number(e.target.value)) })} />
          </label>
          <label className="text-sm text-neutral-300">
            <span className="mb-1 block text-xs text-neutral-400">Max backoff (seconds)</span>
            <input type="number" min={1} className="input" value={draft.retryMaxSecs} disabled={!draft.retryEnabled}
              onChange={(e) => set({ retryMaxSecs: Math.max(1, Number(e.target.value)) })} />
          </label>
        </div>
        <p className="mt-2 text-[11px] text-neutral-500">
          Delay doubles each attempt: {draft.retryBaseSecs}s → {draft.retryBaseSecs * 2}s → {draft.retryBaseSecs * 4}s …, capped at {draft.retryMaxSecs}s.
        </p>
        <label className="mt-4 block text-sm text-neutral-300">
          <span className="mb-1 block text-xs text-neutral-400">Activity log retention (entries)</span>
          <input type="number" min={1} className="input max-w-[200px]" value={draft.activityRetention}
            onChange={(e) => set({ activityRetention: Math.max(1, Number(e.target.value)) })} />
          <span className="mt-1 block text-[11px] text-neutral-500">Older activity entries are pruned beyond this count.</span>
        </label>
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
              const testHook = async () => {
                setTests((t) => ({ ...t, [w.id]: "sending" }));
                try {
                  await api.testWebhook(w);
                  setTests((t) => ({ ...t, [w.id]: { ok: true, msg: "Sent" } }));
                } catch (e) {
                  setTests((t) => ({ ...t, [w.id]: { ok: false, msg: String(e) } }));
                }
              };
              const test = tests[w.id];
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
                    <button className="btn !px-2 !py-1" onClick={testHook} disabled={!w.url || test === "sending"} title="Send a test notification">
                      {test === "sending" ? "Testing…" : "Test"}
                    </button>
                    <button className="btn btn-danger !px-2 !py-1" onClick={removeHook} title="Remove">
                      <AlertTriangle size={13} />
                    </button>
                  </div>
                  {test && test !== "sending" && (
                    <p className={`mb-2 text-[11px] ${test.ok ? "text-emerald-400" : "text-rose-400"}`}>
                      {test.ok ? "✓ Test notification sent." : `✗ ${test.msg}`}
                    </p>
                  )}
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
                  {projects.length > 0 && (
                    <div className="mt-2 border-t border-[var(--color-border)] pt-2">
                      <div className="mb-1 text-[11px] text-neutral-500">
                        Projects {w.projectIds.length === 0 ? "(all)" : `(${w.projectIds.length})`}
                      </div>
                      <div className="flex flex-wrap gap-1.5">
                        {projects.map((p) => {
                          const on = w.projectIds.includes(p.id);
                          return (
                            <button
                              key={p.id}
                              type="button"
                              onClick={() => update({ projectIds: on ? w.projectIds.filter((x) => x !== p.id) : [...w.projectIds, p.id] })}
                              className={`chip border ${on ? "border-indigo-500/50 bg-indigo-600/15 text-indigo-200" : "border-[var(--color-border)] text-neutral-500 hover:text-neutral-300"}`}
                            >
                              {p.name}
                            </button>
                          );
                        })}
                      </div>
                      <p className="mt-1 text-[11px] text-neutral-600">No projects selected = fires for all.</p>
                    </div>
                  )}
                  <div className="mt-2 border-t border-[var(--color-border)] pt-2">
                    <div className="mb-1 text-[11px] text-neutral-500">Message template (optional)</div>
                    <textarea
                      className="input resize-y font-mono text-xs"
                      rows={2}
                      placeholder="{status}: {task} in {project}  ({link})"
                      value={w.template}
                      onChange={(e) => update({ template: e.target.value })}
                    />
                    <p className="mt-1 text-[11px] text-neutral-600">
                      Placeholders: <code className="text-neutral-400">{"{event} {title} {body} {project} {task} {status} {link}"}</code>. Blank = default format.
                    </p>
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
        <div className="mb-3 flex items-center justify-between">
          <h3 className="text-sm font-semibold text-neutral-200">Agents</h3>
          <button className="btn !py-1" onClick={loadHealth} title="Re-detect installed CLIs">
            <RefreshCw size={13} /> Check CLIs
          </button>
        </div>
        <div className="flex flex-col gap-4">
          {AGENTS.map((kind) => {
            const cfg = agentCfg(kind);
            const h = health[kind];
            return (
              <div key={kind} className="rounded-md border border-[var(--color-border)] p-3">
                <div className="mb-2 flex items-center justify-between">
                  <div className="flex items-center gap-2">
                    <span className="text-sm font-medium text-neutral-100">{AGENT_LABELS[kind]}</span>
                    {h && (
                      <span
                        className={`inline-flex items-center gap-1 text-[11px] ${h.available ? "text-emerald-400" : "text-neutral-600"}`}
                        title={h.available ? (h.version ?? "installed") : "not found on PATH"}
                      >
                        {h.available ? <CheckCircle2 size={12} /> : <XCircle size={12} />}
                        {h.available ? (h.version ? h.version.slice(0, 28) : "installed") : "not found"}
                      </span>
                    )}
                  </div>
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

      <section className="card mt-5 p-4">
        <div className="mb-1 flex items-center justify-between">
          <h3 className="text-sm font-semibold text-neutral-200">System diagnostics</h3>
          <button className="btn !py-1" onClick={runDiagnostics} disabled={diagBusy}>
            <Stethoscope size={13} /> {diagBusy ? "Checking…" : "Run diagnostics"}
          </button>
        </div>
        <p className="mb-3 text-xs text-neutral-500">
          Checks agent CLIs, git, the database, and each project's configuration.
        </p>
        {diags === null ? (
          <p className="text-xs text-neutral-600">Run diagnostics to check your environment.</p>
        ) : diags.length === 0 ? (
          <p className="text-xs text-emerald-400">No issues found.</p>
        ) : (
          <div className="flex flex-col gap-1.5">
            {(() => {
              const errors = diags.filter((d) => d.level === "error").length;
              const warns = diags.filter((d) => d.level === "warn").length;
              return (
                <p className="mb-1 text-[11px] text-neutral-500">
                  {errors > 0 && <span className="text-rose-400">{errors} error{errors === 1 ? "" : "s"}</span>}
                  {errors > 0 && warns > 0 && " · "}
                  {warns > 0 && <span className="text-amber-400">{warns} warning{warns === 1 ? "" : "s"}</span>}
                  {errors === 0 && warns === 0 && <span className="text-emerald-400">All checks passed</span>}
                </p>
              );
            })()}
            {diags.map((d, i) => (
              <div key={i} className="flex items-start gap-2 rounded-md border border-[var(--color-border)] bg-[var(--color-surface)] px-2.5 py-1.5 text-sm">
                {d.level === "ok" ? (
                  <CheckCircle2 size={15} className="mt-0.5 shrink-0 text-emerald-400" />
                ) : d.level === "warn" ? (
                  <AlertTriangle size={15} className="mt-0.5 shrink-0 text-amber-400" />
                ) : (
                  <XCircle size={15} className="mt-0.5 shrink-0 text-rose-400" />
                )}
                <div className="min-w-0">
                  <span className="text-neutral-200">{d.name}</span>
                  <span className="text-[11px] text-neutral-600"> · {d.category}</span>
                  <div className="text-xs text-neutral-400">{d.detail}</div>
                </div>
              </div>
            ))}
          </div>
        )}
      </section>

      <section className="card mt-5 p-4">
        <h3 className="mb-1 text-sm font-semibold text-neutral-200">Scheduled backups</h3>
        <p className="mb-3 text-xs text-neutral-500">
          Auto-export your config to a folder on a cadence (keeps the latest 10).
        </p>
        <label className="mb-3 flex items-center gap-2 text-sm text-neutral-300">
          <input type="checkbox" checked={draft.backupEnabled} onChange={(e) => set({ backupEnabled: e.target.checked })} />
          Enable scheduled config backups
        </label>
        <div className="grid grid-cols-2 gap-4">
          <label className="text-sm text-neutral-300">
            <span className="mb-1 block text-xs text-neutral-400">Interval (hours)</span>
            <input type="number" min={1} className="input" value={draft.backupIntervalHours} disabled={!draft.backupEnabled}
              onChange={(e) => set({ backupIntervalHours: Math.max(1, Number(e.target.value)) })} />
          </label>
          <label className="text-sm text-neutral-300">
            <span className="mb-1 block text-xs text-neutral-400">Backup directory</span>
            <input className="input font-mono text-xs" placeholder="/path/to/backups" value={draft.backupDir}
              onChange={(e) => set({ backupDir: e.target.value })} />
          </label>
        </div>
        <div className="mt-3 flex items-center gap-2">
          <button className="btn" onClick={backupNow} disabled={!draft.backupDir.trim()}>Back up now</button>
          {backupNotice && (
            <span className={`text-xs ${backupNotice.ok ? "text-emerald-400" : "text-rose-400"}`}>{backupNotice.msg}</span>
          )}
        </div>
        <p className="mt-2 text-[11px] text-neutral-600">Save settings first so a new directory takes effect for scheduled backups.</p>
      </section>

      <section className="card mt-5 p-4">
        <h3 className="mb-1 text-sm font-semibold text-neutral-200">Import / export configuration</h3>
        <p className="mb-3 text-xs text-neutral-500">
          Move your settings and projects between machines as a portable JSON file. Tasks and history are not included.
        </p>
        <div className="flex items-center gap-2">
          <button className="btn" onClick={exportConfig}>Export config</button>
          <button className="btn" onClick={() => importFileRef.current?.click()}>Import config</button>
          <input
            ref={importFileRef}
            type="file"
            accept="application/json,.json"
            className="hidden"
            onChange={onImportFile}
          />
          {configNotice && (
            <span className={`text-xs ${configNotice.ok ? "text-emerald-400" : "text-rose-400"}`}>{configNotice.msg}</span>
          )}
        </div>
      </section>
    </div>
  );
}
