import { useState } from "react";
import { Moon, Sun, Monitor } from "lucide-react";
import { applyTheme, getTheme, type Theme } from "../lib/theme";

const ORDER: Theme[] = ["dark", "light", "system"];
const META: Record<Theme, { icon: typeof Moon; label: string }> = {
  dark: { icon: Moon, label: "Dark" },
  light: { icon: Sun, label: "Light" },
  system: { icon: Monitor, label: "System" },
};

export function ThemeToggle() {
  const [theme, setTheme] = useState<Theme>(getTheme());
  const cycle = () => {
    const next = ORDER[(ORDER.indexOf(theme) + 1) % ORDER.length];
    applyTheme(next);
    setTheme(next);
  };
  const { icon: Icon, label } = META[theme];
  return (
    <button
      onClick={cycle}
      className="flex w-full items-center gap-2 rounded-md px-2.5 py-1.5 text-xs text-neutral-400 transition-colors hover:bg-[var(--color-surface-2)] hover:text-neutral-200"
      title="Cycle theme (dark / light / system)"
    >
      <Icon size={14} />
      {label} theme
    </button>
  );
}
