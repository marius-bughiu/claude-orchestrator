import { NavLink, Outlet } from "react-router-dom";
import clsx from "clsx";
import { LayoutDashboard, FolderGit2, ListTodo, GanttChartSquare, Clock, Settings as SettingsIcon } from "lucide-react";
import { UsageBar } from "./UsageBar";
import { UpdateBanner } from "./UpdateBanner";
import { UsageAlertBanner } from "./UsageAlertBanner";
import { CommandPalette } from "./CommandPalette";
import { Logo } from "./Logo";
import { ThemeToggle } from "./ThemeToggle";
import { useStore } from "../store";

const NAV = [
  { to: "/dashboard", label: "Dashboard", icon: LayoutDashboard },
  { to: "/projects", label: "Projects", icon: FolderGit2 },
  { to: "/tasks", label: "Tasks", icon: ListTodo },
  { to: "/scheduled", label: "Scheduled", icon: Clock },
  { to: "/timeline", label: "Timeline", icon: GanttChartSquare },
  { to: "/settings", label: "Settings", icon: SettingsIcon },
];

export function Layout() {
  const connected = useStore((s) => s.connected);

  return (
    <div className="flex h-full flex-col">
      <CommandPalette />
      <UsageBar />
      <UpdateBanner />
      <UsageAlertBanner />
      <div className="flex min-h-0 flex-1">
        <nav className="flex w-52 shrink-0 flex-col border-r border-[var(--color-border)] bg-[var(--color-surface)] px-3 py-4">
          <div className="mb-6 flex items-center gap-2 px-2">
            <Logo size={22} />
            <span className="text-sm font-semibold text-neutral-100">Orchestrator</span>
          </div>
          <div className="flex flex-col gap-1">
            {NAV.map(({ to, label, icon: Icon }) => (
              <NavLink
                key={to}
                to={to}
                className={({ isActive }) =>
                  clsx(
                    "flex items-center gap-2.5 rounded-md px-2.5 py-2 text-sm transition-colors",
                    isActive
                      ? "bg-indigo-600/15 text-indigo-200"
                      : "text-neutral-400 hover:bg-[var(--color-surface-2)] hover:text-neutral-200",
                  )
                }
              >
                <Icon size={16} />
                {label}
              </NavLink>
            ))}
          </div>
          <div className="mt-auto px-2">
            <div className="mb-2"><ThemeToggle /></div>
            <div className="text-[11px] text-neutral-600">
              <div className="flex items-center gap-1.5">
                <span
                  className={clsx(
                    "h-1.5 w-1.5 rounded-full",
                    connected ? "bg-emerald-400" : "bg-red-400",
                  )}
                />
                {connected ? "Connected" : "Disconnected"}
              </div>
              <div className="mt-1">Claude Orchestrator v0.1.0</div>
            </div>
          </div>
        </nav>
        <main className="min-w-0 flex-1 overflow-y-auto">
          <Outlet />
        </main>
      </div>
    </div>
  );
}
