import { onCleanup, onMount } from "solid-js";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { api } from "./lib/tauri";
import { setActiveTab, setConfig, setUsage, setProxyStatus } from "./lib/store";
import { BACKGROUND_CHECK_DELAY_MS, runScheduledBackgroundCheck } from "./lib/updater";
import TrayPopover from "./components/TrayPopover";

export default function App() {
  onMount(() => {
    setActiveTab("stats");

    // ESC closes the window.
    const handleKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") getCurrentWindow().hide();
    };
    document.addEventListener("keydown", handleKey);
    onCleanup(() => document.removeEventListener("keydown", handleKey));

    const bgCheckTimer = window.setTimeout(() => {
      runScheduledBackgroundCheck().catch(() => {});
    }, BACKGROUND_CHECK_DELAY_MS);
    onCleanup(() => clearTimeout(bgCheckTimer));

    let disposed = false;
    let poll: ReturnType<typeof setInterval> | undefined;
    const unlisteners: Array<() => void> = [];

    (async () => {
      // Tab events must register before any await - otherwise open-settings / focus-usage from Rust can fire first.
      const [uOpenSettings, uFocusUsage] = await Promise.all([
        listen("open-settings", () => {
          setActiveTab("settings");
        }),
        listen("focus-usage", () => {
          setActiveTab("stats");
        }),
      ]);
      if (disposed) {
        uOpenSettings();
        uFocusUsage();
        return;
      }
      unlisteners.push(uOpenSettings, uFocusUsage);

      const cfg = await api.getConfig();
      setConfig(cfg);

      const status = await api.getProxyStatus();
      setProxyStatus(status);

      const usageData = await api.getUsage();
      setUsage(usageData);

      const [u1, u2, u3] = await Promise.all([
        listen("provider-changed", () => {
          api.getConfig().then(setConfig);
        }),
        listen("usage-updated", () => {
          api.getUsage().then(setUsage);
        }),
        listen("proxy-status-changed", () => {
          api.getProxyStatus().then(setProxyStatus);
        }),
      ]);

      if (disposed) {
        u1();
        u2();
        u3();
        return;
      }

      unlisteners.push(u1, u2, u3);

      poll = setInterval(async () => {
        const u = await api.getUsage();
        setUsage(u);
        const s = await api.getProxyStatus();
        setProxyStatus(s);
      }, 5000);
    })();

    onCleanup(() => {
      disposed = true;
      unlisteners.forEach((u) => u());
      if (poll) clearInterval(poll);
    });
  });

  return <TrayPopover />;
}
