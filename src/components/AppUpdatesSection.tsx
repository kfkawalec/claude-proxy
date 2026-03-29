import { createSignal, onMount } from "solid-js";
import { getVersion } from "@tauri-apps/api/app";
import { checkForUpdateManual, installUpdate } from "../lib/updater";
import { t } from "../lib/i18n";
import Button from "./ui/Button";

const sectionLabel: Record<string, string> = {
  "font-size": "10px",
  "font-weight": "600",
  color: "var(--text-2)",
  "text-transform": "uppercase",
  "letter-spacing": "0.06em",
};

const card: Record<string, string> = {
  background: "var(--bg)",
  "border-radius": "8px",
  padding: "10px 8px",
  display: "flex",
  "flex-direction": "column",
  gap: "8px",
};

/** Przypięte do dołu zakładki Settings (poniżej przewijanej treści). */
export default function AppUpdatesSection() {
  const [appVersion, setAppVersion] = createSignal("");
  const [updateBusy, setUpdateBusy] = createSignal(false);
  const [updateMsg, setUpdateMsg] = createSignal<string | null>(null);

  onMount(async () => {
    setAppVersion(await getVersion().catch(() => ""));
  });

  const handleCheckUpdates = async () => {
    setUpdateBusy(true);
    setUpdateMsg(null);
    try {
      const update = await checkForUpdateManual();
      if (!update) {
        setUpdateMsg(t().settings.noUpdate);
        return;
      }
      setUpdateMsg(t().settings.downloadingUpdate);
      await installUpdate(
        update,
        (pct) => setUpdateMsg(`${t().settings.downloadingUpdate} ${pct}%`),
        () => setUpdateMsg(t().settings.updateReady)
      );
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      setUpdateMsg(`${t().settings.updateFailed}: ${msg}`);
    } finally {
      setUpdateBusy(false);
    }
  };

  return (
    <div
      style={{
        "flex-shrink": "0",
        padding: "10px 8px 10px",
        "border-top": "0.5px solid var(--border)",
        background: "var(--bg-card)",
      }}
    >
      <div style={{ display: "flex", "flex-direction": "column", gap: "6px" }}>
        <div style={sectionLabel}>{t().settings.appUpdates}</div>
        <div style={card}>
          <div style={{ display: "flex", "align-items": "center", "justify-content": "space-between", gap: "8px", "flex-wrap": "wrap" }}>
            <span style={{ "font-size": "12px", color: "var(--text-2)" }}>
              {t().settings.currentVersion}{appVersion() ? ` ${appVersion()}` : ""}
            </span>
            <Button size="sm" disabled={updateBusy()} onClick={handleCheckUpdates}>
              {updateBusy() ? t().settings.checkingUpdates : t().settings.checkUpdates}
            </Button>
          </div>
          {updateMsg() && (
            <div style={{ "font-size": "11px", color: "var(--text-2)", "line-height": "1.4" }}>
              {updateMsg()}
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
