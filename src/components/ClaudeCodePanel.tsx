import { api } from "../lib/tauri";
import { Show, createSignal, onCleanup, onMount } from "solid-js";
import { listen } from "@tauri-apps/api/event";
import { t } from "../lib/i18n";
import Button from "./ui/Button";

const sectionTitle: Record<string, string> = {
  "font-size": "10px",
  "font-weight": "600",
  color: "var(--text-2)",
  "text-transform": "uppercase",
  "letter-spacing": "0.06em",
};

/** Treść w osobnej ramce (panelCard) w TrayPopover. */
const root: Record<string, string> = {
  display: "flex",
  "flex-direction": "column",
  gap: "10px",
  padding: "12px",
  "box-sizing": "border-box",
};

const divider: Record<string, string> = {
  height: "0.5px",
  background: "var(--border)",
  margin: "0",
};

/** Jedna sekcja: status Claude Code, potem integracja proxy w ustawieniach Claude. */
export default function ClaudeCodePanel() {
  const [claudeAuth, setClaudeAuth] = createSignal<boolean | null>(null);
  const [loggingIn, setLoggingIn] = createSignal(false);
  const [installed, setInstalled] = createSignal(false);
  const [busy, setBusy] = createSignal(false);

  const refreshAuth = async () => {
    const auth = await api.checkClaudeAuth().catch(() => false);
    setClaudeAuth(auth);
  };

  const refreshInstall = async () => {
    const s = await api.getClaudeInstallStatus().catch(() => null);
    setInstalled(!!s?.installed);
  };

  onMount(async () => {
    await refreshAuth();
    await refreshInstall();
    const unlisten = await listen("auth-changed", refreshAuth);
    onCleanup(unlisten);
  });

  const handleLogin = async () => {
    setLoggingIn(true);
    await api.claudeLogin().catch(() => {});
    await refreshAuth();
    setLoggingIn(false);
  };

  const toggleInstall = async () => {
    if (busy()) return;
    setBusy(true);
    if (installed()) await api.uninstallClaudeProxySettings().catch(() => {});
    else await api.installClaudeProxySettings().catch(() => {});
    await refreshInstall();
    setBusy(false);
  };

  return (
    <div style={root} role="region" aria-label={t().layout.claudeCodeSection}>
      <div style={{ ...sectionTitle, "margin-bottom": "2px" }}>{t().layout.claudeCodeSection}</div>

      <div style={{ display: "flex", "flex-direction": "column", gap: "8px" }}>
        <div style={{ display: "flex", "align-items": "center", "justify-content": "space-between", gap: "6px" }}>
          <div style={{ display: "flex", "align-items": "center", gap: "6px", "min-width": "0" }}>
            <div
              style={{
                width: "6px",
                height: "6px",
                "border-radius": "50%",
                background: claudeAuth() === null ? "var(--text-3)" : claudeAuth() ? "var(--green)" : "var(--orange)",
                "flex-shrink": "0",
              }}
            />
            <span style={{ "font-size": "12px", "font-weight": "500" }}>
              {claudeAuth() === null ? "…" : claudeAuth() ? t().settings.authOk : t().settings.authNone}
            </span>
          </div>
          {!claudeAuth() && (
            <Button size="sm" disabled={loggingIn()} onClick={handleLogin}>
              {loggingIn() ? t().settings.loggingIn : t().settings.loginBtn}
            </Button>
          )}
        </div>
        <div style={{ "font-size": "10px", color: "var(--text-2)", "line-height": "1.35" }}>
          {t().settings.authHint}
        </div>
      </div>

      <div style={divider} />

      <Show
        when={installed()}
        fallback={
          <div
            style={{
              padding: "10px 10px 10px 11px",
              "border-radius": "8px",
              border: "0.5px solid var(--border)",
              "border-left": "3px solid var(--accent)",
              background: "var(--bg)",
              display: "flex",
              "flex-direction": "column",
              gap: "10px",
              "box-sizing": "border-box",
            }}
            role="status"
          >
            <div>
              <div
                style={{
                  "font-size": "12px",
                  "font-weight": "600",
                  color: "var(--text-1)",
                  "line-height": "1.3",
                  "margin-bottom": "4px",
                }}
              >
                {t().status.installBannerTitle}
              </div>
              <div style={{ "font-size": "10px", color: "var(--text-2)", "line-height": "1.45" }}>
                {t().status.installBannerHint}
              </div>
            </div>
            <Button
              disabled={busy()}
              size="md"
              variant="primary"
              fullWidth
              onClick={toggleInstall}
            >
              {busy() ? t().status.busy : t().status.installButtonCta}
            </Button>
          </div>
        }
      >
        <div
          style={{
            display: "flex",
            "align-items": "center",
            "justify-content": "space-between",
            gap: "10px",
            "flex-wrap": "wrap",
          }}
        >
          <span style={{ "font-size": "11px", color: "var(--text-2)", "line-height": "1.35" }}>{t().status.installed}</span>
          <Button disabled={busy()} size="sm" variant="danger" onClick={toggleInstall}>
            {busy() ? t().status.busy : t().status.uninstall}
          </Button>
        </div>
      </Show>
    </div>
  );
}
