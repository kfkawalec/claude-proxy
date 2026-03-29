import { createSignal } from "solid-js";
import type { AppConfig, UsageData, ProxyStatus } from "./tauri";

export const [config, setConfig] = createSignal<AppConfig | null>(null);
export const [usage, setUsage] = createSignal<UsageData | null>(null);
export const [proxyStatus, setProxyStatus] = createSignal<ProxyStatus>("Stopped");
export const [activeTab, setActiveTab] = createSignal<"settings" | "stats">("stats");
export const [models, setModels] = createSignal<any[]>([]);
export const [toastMessage, setToastMessage] = createSignal<string | null>(null);
/** Ustawiane po tle `check()` — baner z propozycją aktualizacji. */
export const [pendingUpdateVersion, setPendingUpdateVersion] = createSignal<string | null>(null);

let toastTimer: ReturnType<typeof setTimeout> | null = null;
export function showToast(message: string, ms = 1800) {
  setToastMessage(message);
  if (toastTimer) clearTimeout(toastTimer);
  toastTimer = setTimeout(() => {
    setToastMessage(null);
    toastTimer = null;
  }, ms);
}
