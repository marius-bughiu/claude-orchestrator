import { useEffect, useState } from "react";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { Download, Loader2, X, AlertTriangle } from "lucide-react";
import * as api from "../api";

type Phase = "idle" | "available" | "draining" | "installing" | "error";

/// Detects a new release, informs the user, and on confirmation drains the
/// scheduler (no new jobs, wait for running ones), then installs and relaunches.
/// Also checks once on launch.
export function UpdateBanner() {
  const [update, setUpdate] = useState<Update | null>(null);
  const [phase, setPhase] = useState<Phase>("idle");
  const [remaining, setRemaining] = useState(0);
  const [message, setMessage] = useState("");
  const [dismissed, setDismissed] = useState(false);

  useEffect(() => {
    let active = true;
    // Runs only inside the Tauri host; in a browser this throws and we stay idle.
    check()
      .then((u) => {
        if (active && u) {
          setUpdate(u);
          setPhase("available");
        }
      })
      .catch(() => {});
    return () => {
      active = false;
    };
  }, []);

  const startUpdate = async () => {
    if (!update) return;
    try {
      setPhase("draining");
      setMessage("Finishing active sessions before updating…");
      await api.beginDrain();

      // Wait for in-flight sessions to complete.
      await new Promise<void>((resolve) => {
        const tick = async () => {
          try {
            const status = await api.getStatus();
            setRemaining(status.activeSessions);
            if (status.activeSessions <= 0) {
              clearInterval(timer);
              resolve();
            }
          } catch {
            clearInterval(timer);
            resolve();
          }
        };
        const timer = setInterval(tick, 1500);
        tick();
      });

      setPhase("installing");
      setMessage("Downloading and installing the update…");
      await update.downloadAndInstall();
      await relaunch();
    } catch (e) {
      setMessage(String(e));
      setPhase("error");
      // Re-enable scheduling if the update failed.
      api.cancelDrain().catch(() => {});
    }
  };

  const dismiss = async () => {
    setDismissed(true);
    if (phase === "draining") await api.cancelDrain().catch(() => {});
  };

  if (phase === "idle" || dismissed || !update) return null;

  return (
    <div className="flex items-center gap-3 border-b border-indigo-500/30 bg-indigo-600/10 px-4 py-2 text-sm text-indigo-100">
      {phase === "error" ? (
        <AlertTriangle size={16} className="text-red-300" />
      ) : phase === "available" ? (
        <Download size={16} className="text-indigo-300" />
      ) : (
        <Loader2 size={16} className="animate-spin text-indigo-300" />
      )}

      <div className="min-w-0 flex-1">
        {phase === "available" && (
          <span>
            Version <span className="font-semibold">{update.version}</span> is available.
            {update.body ? <span className="ml-2 text-indigo-300/80">{update.body.slice(0, 80)}</span> : null}
          </span>
        )}
        {phase === "draining" && (
          <span>{message} {remaining > 0 ? `(${remaining} running)` : ""}</span>
        )}
        {phase === "installing" && <span>{message}</span>}
        {phase === "error" && <span className="text-red-200">Update failed: {message}</span>}
      </div>

      {phase === "available" && (
        <button className="btn btn-primary !py-1" onClick={startUpdate}>
          <Download size={14} /> Update &amp; restart
        </button>
      )}
      {(phase === "available" || phase === "error") && (
        <button className="text-indigo-300 hover:text-white" onClick={dismiss} title="Later">
          <X size={16} />
        </button>
      )}
    </div>
  );
}
