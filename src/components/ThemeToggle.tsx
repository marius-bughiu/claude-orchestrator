import { useState } from "react";
import { Moon, Sun } from "lucide-react";
import { applyTheme, getTheme, type Theme } from "../lib/theme";

export function ThemeToggle() {
  const [theme, setTheme] = useState<Theme>(getTheme());
  const toggle = () => {
    const next: Theme = theme === "dark" ? "light" : "dark";
    applyTheme(next);
    setTheme(next);
  };
  return (
    <button
      onClick={toggle}
      className="flex w-full items-center gap-2 rounded-md px-2.5 py-1.5 text-xs text-neutral-400 transition-colors hover:bg-[var(--color-surface-2)] hover:text-neutral-200"
      title={`Switch to ${theme === "dark" ? "light" : "dark"} theme`}
    >
      {theme === "dark" ? <Moon size={14} /> : <Sun size={14} />}
      {theme === "dark" ? "Dark" : "Light"} theme
    </button>
  );
}
