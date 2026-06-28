import {
  isPermissionGranted,
  requestPermission,
  sendNotification,
} from "@tauri-apps/plugin-notification";

let granted: boolean | null = null;

/// Send a desktop notification, requesting permission once. No-ops outside the
/// Tauri host or if permission is denied.
export async function notify(title: string, body: string): Promise<void> {
  try {
    if (granted === null) {
      granted = await isPermissionGranted();
      if (!granted) granted = (await requestPermission()) === "granted";
    }
    if (granted) sendNotification({ title, body });
  } catch {
    // Not running in Tauri, or notifications unavailable — ignore.
  }
}
