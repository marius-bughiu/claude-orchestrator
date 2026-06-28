export type Theme = "dark" | "light" | "system";

const KEY = "orchestrator.theme";

export function getTheme(): Theme {
  const saved = localStorage.getItem(KEY);
  return saved === "light" || saved === "system" ? saved : "dark";
}

function prefersDark(): boolean {
  return window.matchMedia?.("(prefers-color-scheme: dark)").matches ?? true;
}

/// Resolve "system" to the actual dark/light the OS prefers.
export function effectiveTheme(theme: Theme): "dark" | "light" {
  if (theme === "system") return prefersDark() ? "dark" : "light";
  return theme;
}

function paint(theme: Theme): void {
  const root = document.documentElement;
  const eff = effectiveTheme(theme);
  root.classList.toggle("light", eff === "light");
  root.classList.toggle("dark", eff === "dark");
}

export function applyTheme(theme: Theme): void {
  localStorage.setItem(KEY, theme);
  paint(theme);
}

export function initTheme(): void {
  paint(getTheme());
  // Repaint on OS theme changes while in "system" mode.
  window.matchMedia?.("(prefers-color-scheme: dark)").addEventListener?.("change", () => {
    if (getTheme() === "system") paint("system");
  });
}
