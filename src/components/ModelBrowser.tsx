import { config, models, setConfig, setModels } from "../lib/store";
import { api } from "../lib/tauri";
import { onMount, For, Show, createSignal, createMemo } from "solid-js";
import { t, fmtTemplate } from "../lib/i18n";
import { hubDisplayName } from "../lib/hubDisplayName";

type ErrorKind = "not_configured" | "network" | null;

const sectionLabel: Record<string, string> = {
  "font-size": "10px",
  "font-weight": "600",
  color: "var(--text-2)",
  "text-transform": "uppercase",
  "letter-spacing": "0.06em",
};

function isConfigError(msg: string) {
  return msg.toLowerCase().includes("not configured") ||
         msg.toLowerCase().includes("endpoint") ||
         msg.toLowerCase().includes("api key") ||
         msg.toLowerCase().includes("invoke");
}

export default function ModelBrowser() {
  const hubName = () => hubDisplayName(config());

  const [errorKind, setErrorKind] = createSignal<ErrorKind>(null);
  const [errorMsg, setErrorMsg] = createSignal<string | null>(null);
  const [persisting, setPersisting] = createSignal(false);
  const [justSaved, setJustSaved] = createSignal(false);
  const [mapping, setMapping] = createSignal<Record<string, string>>({
    claude_opus: "",
    claude_sonnet: "",
    claude_haiku: "",
  });

  const sortedModels = createMemo(() =>
    [...models()].sort((a, b) => {
      const left = String(a.id ?? a.model ?? "").toLowerCase();
      const right = String(b.id ?? b.model ?? "").toLowerCase();
      return left.localeCompare(right);
    })
  );

  const loadModels = async () => {
    setErrorKind(null);
    setErrorMsg(null);
    try {
      const fetched = await api.fetchModels();
      setModels(fetched);
    } catch (e: any) {
      const msg = typeof e === "string" ? e : (e?.message ?? "");
      if (isConfigError(msg)) {
        setErrorKind("not_configured");
      } else {
        setErrorKind("network");
        setErrorMsg(msg || "Unknown error");
      }
    }
  };

  const loadMapping = async () => {
    const cfg = await api.getConfig().catch(() => null);
    if (!cfg) return;
    setConfig(cfg);
    setMapping({
      claude_opus: cfg.model_overrides?.claude_opus ?? "",
      claude_sonnet: cfg.model_overrides?.claude_sonnet ?? "",
      claude_haiku: cfg.model_overrides?.claude_haiku ?? "",
    });
  };

  const persistMapping = async (next: Record<string, string>) => {
    if (errorKind() === "not_configured") return;
    setPersisting(true);
    const cfg = await api.getConfig().catch(() => null);
    if (!cfg) {
      setPersisting(false);
      return;
    }
    const nextOverrides = { ...(cfg.model_overrides ?? {}) } as Record<string, string>;
    for (const key of ["claude_opus", "claude_sonnet", "claude_haiku"] as const) {
      const value = next[key]?.trim();
      if (value) nextOverrides[key] = value;
      else delete nextOverrides[key];
    }
    cfg.model_overrides = nextOverrides;
    await api.saveSettings(cfg).catch(() => {});
    setConfig(cfg);
    setPersisting(false);
    setJustSaved(true);
    setTimeout(() => setJustSaved(false), 1500);
  };

  onMount(async () => {
    await loadModels();
    await loadMapping();
  });

  return (
    <div style={{ display: "flex", "flex-direction": "column", "flex-shrink": "0" }}>
      <div style={{ padding: "0", display: "flex", "flex-direction": "column", gap: "8px" }}>

      {/* Not configured */}
      <Show when={errorKind() === "not_configured"}>
        <div style={{
          background: "var(--bg-card)",
          "border-radius": "9px",
          padding: "12px",
          border: "0.5px solid var(--border)",
          "box-shadow": "var(--shadow-card)",
          display: "flex",
          gap: "10px",
          "align-items": "flex-start",
        }}>
          <div style={{
            width: "28px", height: "28px", "border-radius": "7px",
            background: "var(--bg-seg)", display: "flex", "align-items": "center",
            "justify-content": "center", "flex-shrink": "0",
          }}>
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="var(--text-2)" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
              <circle cx="12" cy="12" r="10"/><line x1="12" y1="8" x2="12" y2="12"/><line x1="12" y1="16" x2="12.01" y2="16"/>
            </svg>
          </div>
          <div>
            <div style={{ "font-size": "12px", "font-weight": "500", "margin-bottom": "3px" }}>
              {fmtTemplate(t().models.notConfigured, { hubName: hubName() })}
            </div>
            <div style={{ "font-size": "11px", color: "var(--text-2)", "line-height": "1.4" }}>
              {t().models.notConfiguredHint}
            </div>
          </div>
        </div>
      </Show>

      {/* Network / other error */}
      <Show when={errorKind() === "network"}>
        <div style={{ "font-size": "11px", color: "var(--red)", background: "var(--bg-card)", "border-radius": "7px", padding: "8px 10px", border: "0.5px solid var(--border)" }}>
          {errorMsg()}
        </div>
      </Show>

        <Show when={errorKind() !== "not_configured"}>
          <div style={{ display: "flex", "flex-direction": "column", gap: "6px" }}>
            <div style={{ display: "flex", "align-items": "baseline", "justify-content": "space-between", gap: "8px" }}>
              <div style={sectionLabel}>{fmtTemplate(t().models.mapTitle, { hubName: hubName() })}</div>
              <Show when={justSaved()}>
                <span style={{ "font-size": "11px", color: "var(--green)", "font-weight": "500", "flex-shrink": "0" }}>{t().settings.saved}</span>
              </Show>
            </div>
          <div style={{ background: "var(--bg)", "border-radius": "8px", padding: "10px 8px" }}>
          <div style={{ "font-size": "11px", color: "var(--text-2)", "line-height": "1.4", "margin-bottom": "8px" }}>
            {t().models.mapHint}
          </div>

          <div style={{ display: "grid", gap: "7px" }}>
            <label style={{ display: "grid", gap: "4px" }}>
              <span style={{ "font-size": "11px", color: "var(--text-2)" }}>{t().models.claudeOpus}</span>
              <select
                value={mapping().claude_opus}
                onChange={(e) => {
                  const v = e.currentTarget.value;
                  const next = { ...mapping(), claude_opus: v };
                  setMapping(next);
                  void persistMapping(next);
                }}
                disabled={persisting()}
                style={{ "font-size": "12px", padding: "7px 8px", "border-radius": "7px", border: "0.5px solid var(--border)", background: "var(--bg)" }}
              >
                <option value="">{t().models.selectModel}</option>
                <For each={sortedModels()}>{(m) => <option value={m.id ?? m.model}>{m.id ?? m.model}</option>}</For>
              </select>
            </label>

            <label style={{ display: "grid", gap: "4px" }}>
              <span style={{ "font-size": "11px", color: "var(--text-2)" }}>{t().models.claudeSonnet}</span>
              <select
                value={mapping().claude_sonnet}
                onChange={(e) => {
                  const v = e.currentTarget.value;
                  const next = { ...mapping(), claude_sonnet: v };
                  setMapping(next);
                  void persistMapping(next);
                }}
                disabled={persisting()}
                style={{ "font-size": "12px", padding: "7px 8px", "border-radius": "7px", border: "0.5px solid var(--border)", background: "var(--bg)" }}
              >
                <option value="">{t().models.selectModel}</option>
                <For each={sortedModels()}>{(m) => <option value={m.id ?? m.model}>{m.id ?? m.model}</option>}</For>
              </select>
            </label>

            <label style={{ display: "grid", gap: "4px" }}>
              <span style={{ "font-size": "11px", color: "var(--text-2)" }}>{t().models.claudeHaiku}</span>
              <select
                value={mapping().claude_haiku}
                onChange={(e) => {
                  const v = e.currentTarget.value;
                  const next = { ...mapping(), claude_haiku: v };
                  setMapping(next);
                  void persistMapping(next);
                }}
                disabled={persisting()}
                style={{ "font-size": "12px", padding: "7px 8px", "border-radius": "7px", border: "0.5px solid var(--border)", background: "var(--bg)" }}
              >
                <option value="">{t().models.selectModel}</option>
                <For each={sortedModels()}>{(m) => <option value={m.id ?? m.model}>{m.id ?? m.model}</option>}</For>
              </select>
            </label>
          </div>

          </div>
          </div>
        </Show>
      </div>
    </div>
  );
}
