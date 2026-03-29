import { Show, createSignal, onCleanup, onMount } from "solid-js";
import { activeTab, pendingUpdateVersion, setActiveTab, showToast, toastMessage } from "../lib/store";
import { dismissPendingUpdate, getCachedPendingUpdate, installUpdate } from "../lib/updater";
import { t } from "../lib/i18n";
import ProviderSwitch from "./ProviderSwitch";
import UsageDashboard from "./UsageDashboard";
import ModelBrowser from "./ModelBrowser";
import SettingsForm from "./SettingsForm";
import AppUpdatesSection from "./AppUpdatesSection";
import ProxyActivityStrip from "./ProxyActivityStrip";
import ClaudeCodePanel from "./ClaudeCodePanel";
import SegmentedControl from "./ui/SegmentedControl";
import Button from "./ui/Button";

/** Kolumna bez ramki — ramki są tylko wewnątrz (panelCard). */
const column: Record<string, string> = {
  flex: "1 1 0",
  width: "50%",
  "min-width": "0",
  display: "flex",
  "flex-direction": "column",
  gap: "10px",
  height: "100%",
  "min-height": "0",
};

const panelCard: Record<string, string> = {
  border: "0.5px solid var(--border)",
  "border-radius": "10px",
  background: "var(--bg-card)",
  overflow: "hidden",
  display: "flex",
  "flex-direction": "column",
  "min-width": "0",
  "box-sizing": "border-box",
};

/** Lista żądań: zajmuje wolną przestrzeń w górnej karcie (Claude Code jest przypięty do dołu kolumny). */
const leftActivitySlot: Record<string, string> = {
  flex: "1 1 0",
  "min-height": "0",
  display: "flex",
  "flex-direction": "column",
  overflow: "hidden",
};


export default function TrayPopover() {
  const [bannerBusy, setBannerBusy] = createSignal(false);
  const [bannerProgress, setBannerProgress] = createSignal<string | null>(null);

  const handleBannerUpdate = async () => {
    const u = getCachedPendingUpdate();
    if (!u) return;
    setBannerBusy(true);
    setBannerProgress(null);
    try {
      await installUpdate(
        u,
        (pct) => setBannerProgress(`${t().settings.downloadingUpdate} ${pct}%`),
        () => setBannerProgress(t().settings.updateReady)
      );
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      showToast(`${t().settings.updateFailed}: ${msg}`, 4000);
      setBannerBusy(false);
      setBannerProgress(null);
    }
  };

  const handleBannerDismiss = () => {
    if (bannerBusy()) return;
    setBannerProgress(null);
    const v = pendingUpdateVersion();
    if (v) dismissPendingUpdate(v);
  };

  onMount(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      if (activeTab() !== "settings") return;
      e.preventDefault();
      setActiveTab("stats");
    };
    window.addEventListener("keydown", onKeyDown);
    onCleanup(() => window.removeEventListener("keydown", onKeyDown));
  });

  return (
    <div style={{ display: "flex", "flex-direction": "column", height: "100%", position: "relative" }}>
      <Show when={pendingUpdateVersion()}>
        <div
          style={{
            margin: "0 12px 8px",
            padding: "8px 10px",
            "border-radius": "8px",
            background: "var(--bg-card)",
            border: "0.5px solid var(--accent)",
            display: "flex",
            "flex-direction": "column",
            gap: "8px",
          }}
        >
          <div style={{ "font-size": "11px", color: "var(--text-1)", "line-height": "1.35", "font-weight": "500" }}>
            {bannerProgress() ?? t().settings.updateBanner.replace("{{version}}", pendingUpdateVersion() ?? "")}
          </div>
          <div style={{ display: "flex", "justify-content": "flex-end", gap: "6px", "flex-wrap": "wrap" }}>
            <Button size="sm" disabled={bannerBusy()} onClick={handleBannerDismiss}>
              {t().settings.updateLater}
            </Button>
            <Button size="sm" variant="primary" disabled={bannerBusy()} onClick={handleBannerUpdate}>
              {bannerBusy() ? t().settings.downloadingUpdate : t().settings.updateNow}
            </Button>
          </div>
        </div>
      </Show>

      <div style={{ flex: "1", "min-height": "0", display: "flex", "flex-direction": "row", gap: "12px", padding: "0 12px 12px", "box-sizing": "border-box" }}>
        <div style={column}>
          {/* Ramka 1: Active provider + Recent requests — rozciąga się, żeby wolna wysokość była tu, nie pod Claude */}
          <div style={{ ...panelCard, flex: "1 1 0", "min-height": "0" }}>
            <div style={{ padding: "10px 8px 0", "flex-shrink": "0" }}>
              <div style={{ "font-size": "10px", "font-weight": "600", color: "var(--text-2)", "text-transform": "uppercase", "letter-spacing": "0.06em", "margin-bottom": "6px" }}>
                {t().layout.activeProvider}
              </div>
              <ProviderSwitch compact />
            </div>
            <div style={{ height: "0.5px", background: "var(--border)", margin: "0 8px", "flex-shrink": "0" }} />
            <div style={{ ...leftActivitySlot, padding: "8px 8px 10px" }}>
              <ProxyActivityStrip />
            </div>
          </div>

          {/* Ramka 2: Claude Code — tylko wysokość treści, zawsze przy dolnej krawędzi kolumny */}
          <div style={{ ...panelCard, flex: "0 0 auto", "flex-shrink": "0", overflow: "auto" }}>
            <ClaudeCodePanel />
          </div>
        </div>

        {/* Ramka 3: cały prawy panel (zakładki + treść) */}
        <div style={{ ...column, gap: "0" }}>
          <div style={{ ...panelCard, flex: "1", "min-height": "0" }}>
            <div
              style={{
                "flex-shrink": "0",
                padding: "10px 12px",
                "border-bottom": "0.5px solid var(--border)",
                background: "var(--bg)",
              }}
            >
              <SegmentedControl
                options={[
                  { value: "stats", label: t().tabs.stats },
                  { value: "settings", label: t().tabs.settings },
                ]}
                value={activeTab()}
                onChange={(v) => setActiveTab(v as "settings" | "stats")}
              />
            </div>
            <div
              style={{
                flex: "1",
                "min-height": "0",
                overflow: "hidden",
                display: "flex",
                "flex-direction": "column",
              }}
            >
              <Show when={activeTab() === "stats"}>
                <div style={{ flex: "1", "min-height": "0", display: "flex", "flex-direction": "column", overflow: "hidden" }}>
                  <UsageDashboard />
                </div>
              </Show>
              <Show when={activeTab() === "settings"}>
                <div
                  style={{
                    flex: "1",
                    "min-height": "0",
                    display: "flex",
                    "flex-direction": "column",
                    overflow: "hidden",
                    "box-sizing": "border-box",
                  }}
                >
                  <div
                    style={{
                      flex: "1",
                      "min-height": "0",
                      overflow: "auto",
                      padding: "10px 8px 12px",
                      display: "flex",
                      "flex-direction": "column",
                      gap: "14px",
                      "box-sizing": "border-box",
                    }}
                  >
                    <ModelBrowser />
                    <SettingsForm />
                  </div>
                  <AppUpdatesSection />
                </div>
              </Show>
            </div>
          </div>
        </div>
      </div>

      <Show when={toastMessage()}>
        <div
          style={{
            position: "absolute",
            bottom: "10px",
            left: "50%",
            transform: "translateX(-50%)",
            "max-width": "85%",
            padding: "8px 12px",
            "border-radius": "999px",
            background: "rgba(20,20,22,0.92)",
            color: "#fff",
            "font-size": "11px",
            "font-weight": "500",
            "letter-spacing": "0.01em",
            "box-shadow": "0 8px 24px rgba(0,0,0,0.25)",
            "z-index": "50",
            "pointer-events": "none",
            "white-space": "nowrap",
            overflow: "hidden",
            "text-overflow": "ellipsis",
          }}
        >
          {toastMessage()}
        </div>
      </Show>
    </div>
  );
}
