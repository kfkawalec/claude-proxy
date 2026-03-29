import { check, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { setPendingUpdateVersion } from "./store";

const STORAGE_LAST_CHECK = "claude-proxy-updater-last-check";
const STORAGE_DISMISSED_VERSION = "claude-proxy-updater-dismissed";

/** Co ile najwcześniej ponownie odpytać serwer (oszczędza ruch, nie obciąża GitHub przy każdym otwarciu). */
export const CHECK_INTERVAL_MS = 24 * 60 * 60 * 1000;

/** Opóźnienie pierwszego tła checku po starcie — UI i tray są gotowe wcześniej. */
export const BACKGROUND_CHECK_DELAY_MS = 4000;

let cachedUpdate: Update | null = null;

export function getCachedPendingUpdate(): Update | null {
  return cachedUpdate;
}

export function clearPendingUpdate(): void {
  cachedUpdate = null;
  setPendingUpdateVersion(null);
}

/** „Później” dla danej wersji — nie pokazuj banera, aż nie pojawi się nowszy tag. */
export function dismissPendingUpdate(version: string): void {
  localStorage.setItem(STORAGE_DISMISSED_VERSION, version);
  clearPendingUpdate();
}

export function shouldRunScheduledCheck(): boolean {
  const last = Number(localStorage.getItem(STORAGE_LAST_CHECK) || "0");
  return Date.now() - last >= CHECK_INTERVAL_MS;
}

function recordCheckAttempt(): void {
  localStorage.setItem(STORAGE_LAST_CHECK, String(Date.now()));
}

/**
 * Jedno lekkie sprawdzenie (GET latest.json) — wywołuj max. raz / CHECK_INTERVAL_MS.
 * Przy dostępnej nowszej wersji ustawia baner (cache + signal).
 */
export async function runScheduledBackgroundCheck(): Promise<void> {
  if (!shouldRunScheduledCheck()) return;
  try {
    const u = await check({ timeout: 90_000 });
    recordCheckAttempt();
    if (!u) return;
    if (localStorage.getItem(STORAGE_DISMISSED_VERSION) === u.version) return;
    cachedUpdate = u;
    setPendingUpdateVersion(u.version);
  } catch {
    recordCheckAttempt();
  }
}

export async function checkForUpdateManual(): Promise<Update | null> {
  return check({ timeout: 120_000 });
}

export async function installUpdate(
  update: Update,
  onProgress: (pct: number | null) => void,
  onRelaunching: () => void
): Promise<void> {
  let downloaded = 0;
  let total = 0;
  await update.downloadAndInstall((event) => {
    if (event.event === "Started") {
      total = event.data.contentLength ?? 0;
    }
    if (event.event === "Progress") {
      downloaded += event.data.chunkLength;
      if (total > 0) {
        const pct = Math.round((downloaded / total) * 100);
        onProgress(pct);
      }
    }
  });
  clearPendingUpdate();
  localStorage.removeItem(STORAGE_DISMISSED_VERSION);
  onRelaunching();
  await relaunch();
}
