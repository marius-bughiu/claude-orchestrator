import type { AgentKind } from "../api/types";

const SUGGESTIONS: Record<AgentKind, string[]> = {
  claude: ["opus", "sonnet", "haiku"],
  gemini: ["gemini-2.5-pro", "gemini-2.5-flash"],
  codex: ["gpt-5-codex", "o4-mini"],
};

/// A free-text model input with per-agent suggestions. Empty = agent default
/// (latest Opus for Claude; each CLI's own latest otherwise).
export function ModelInput({
  agent,
  value,
  onChange,
  id = "model-input",
}: {
  agent: AgentKind;
  value: string;
  onChange: (v: string) => void;
  id?: string;
}) {
  return (
    <>
      <input
        className="input"
        list={`${id}-list`}
        placeholder="default (latest)"
        value={value}
        onChange={(e) => onChange(e.target.value)}
      />
      <datalist id={`${id}-list`}>
        {SUGGESTIONS[agent].map((m) => (
          <option key={m} value={m} />
        ))}
      </datalist>
    </>
  );
}
