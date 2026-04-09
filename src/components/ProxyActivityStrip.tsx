import { For, Show, createMemo, createSignal, onCleanup, onMount } from "solid-js";
import { listen } from "@tauri-apps/api/event";
import { api, type ProxyActivityEntry } from "../lib/tauri";
import { activeTab, setActiveTab } from "../lib/store";
import { t, fmtTemplate } from "../lib/i18n";

function fmtTime(tsMs: number): string {
  const d = new Date(tsMs);
  if (Number.isNaN(d.getTime())) return "";
  return d.toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit", second: "2-digit" });
}

function fmtDuration(ms: number | undefined): string {
  const n = Number(ms ?? 0);
  if (!Number.isFinite(n) || n <= 0) return "";
  if (n < 1000) return `${Math.round(n)}ms`;
  return `${(n / 1000).toFixed(1)}s`;
}

function fmtTokens(e: ProxyActivityEntry): string {
  const inn = Number(e.input_tokens ?? 0);
  const out = Number(e.output_tokens ?? 0);
  if (inn <= 0 && out <= 0) return "";
  return `in ${inn.toLocaleString()} · out ${out.toLocaleString()}`;
}

function statusLineColor(status: number): string {
  if (status >= 400) return "var(--orange)";
  if (status >= 300) return "var(--text-2)";
  return "var(--text-1)";
}

export default function ProxyActivityStrip() {
  const [rows, setRows] = createSignal<ProxyActivityEntry[]>([]);

  const logSummary = createMemo(() => {
    const r = rows();
    if (r.length === 0) return "";
    let inTok = 0;
    let outTok = 0;
    for (const e of r) {
      inTok += Number(e.input_tokens ?? 0);
      outTok += Number(e.output_tokens ?? 0);
    }
    return fmtTemplate(t().settings.proxyActivityLogSummary, {
      count: String(r.length),
      inTok: inTok.toLocaleString(),
      outTok: outTok.toLocaleString(),
    });
  });

  const load = async () => {
    const r = await api.getProxyActivity().catch(() => []);
    setRows(r);
  };

  onMount(async () => {
    await load();
    const u1 = await listen("proxy-activity", () => {
      void load();
    });
    const u2 = await listen("usage-updated", () => {
      void load();
    });
    onCleanup(() => {
      u1();
      u2();
    });
  });

  return (
    <div
      style={{
        display: "flex",
        "flex-direction": "column",
        height: "100%",
        "min-height": "0",
        margin: "0 10px",
        width: "calc(100% - 20px)",
        overflow: "hidden",
        "border-radius": "7px",
        background: "var(--bg)",
        padding: "6px 0",
        "box-sizing": "border-box",
      }}
    >
      <div
        style={{
          display: "flex",
          "align-items": "baseline",
          "justify-content": "space-between",
          gap: "8px",
          "flex-shrink": "0",
          "margin-bottom": "4px",
          "min-width": "0",
        }}
      >
        <div style={{ "font-size": "10px", "font-weight": "600", color: "var(--text-2)" }}>
          {t().settings.proxyActivity}
        </div>
        <Show when={activeTab() === "settings"}>
          <button
            type="button"
            onClick={() => setActiveTab("stats")}
            style={{
              "font-size": "10px",
              "font-weight": "600",
              color: "var(--accent)",
              background: "transparent",
              border: "none",
              padding: "0",
              cursor: "pointer",
              "flex-shrink": "0",
              "white-space": "nowrap",
            }}
          >
            {t().settings.proxyActivityGoStats} →
          </button>
        </Show>
      </div>
      <Show when={rows().length > 0}>
        <div
          style={{
            "font-size": "9px",
            color: "var(--text-3)",
            "line-height": "1.35",
            "margin-bottom": "4px",
            "flex-shrink": "0",
          }}
        >
          {logSummary()}
        </div>
      </Show>
      {rows().length === 0 ? (
        <div style={{ "font-size": "10px", color: "var(--text-3)" }}>{t().settings.proxyActivityEmpty}</div>
      ) : (
        <div
          style={{
            flex: "1",
            "min-height": "0",
            "min-width": "0",
            width: "100%",
            "overflow-x": "auto",
            "overflow-y": "auto",
            "WebkitOverflowScrolling": "touch",
          }}
        >
          <div style={{ display: "flex", "flex-direction": "column", gap: "4px", width: "max-content", "min-width": "100%", "box-sizing": "border-box" }}>
            <For each={rows()}>
              {(e) => {
                const dur = fmtDuration(e.duration_ms);
                const tok = fmtTokens(e);
                const errText = (e.error_detail ?? "").trim();
                return (
                  <div style={{ display: "flex", "flex-direction": "column", gap: "2px", "max-width": "none" }}>
                    <div
                      style={{
                        "font-size": "10px",
                        color: "var(--text-1)",
                        "line-height": "1.35",
                        "font-variant-numeric": "tabular-nums",
                        "white-space": "nowrap",
                      }}
                    >
                      <span style={{ color: "var(--text-2)" }}>{fmtTime(e.ts_ms)}</span>
                      {" "}
                      <span style={{ color: statusLineColor(e.status) }}>{e.status}</span>
                      {" "}
                      <span style={{ color: "var(--accent)" }}>{e.provider}</span>
                      {" "}
                      {e.model ? <span style={{ color: "var(--text-2)" }}>{e.model}</span> : null}
                      {" "}
                      <span style={{ color: "var(--text-3)" }}>{e.path}</span>
                      {dur ? (
                        <>
                          {" "}
                          <span style={{ color: "var(--text-3)" }}>· {dur}</span>
                        </>
                      ) : null}
                      {tok ? (
                        <>
                          {" "}
                          <span style={{ color: "var(--text-3)" }}>· {tok}</span>
                        </>
                      ) : null}
                    </div>
                    <Show when={errText.length > 0}>
                      <div
                        style={{
                          "font-size": "9px",
                          color: "var(--orange)",
                          "line-height": "1.35",
                          "white-space": "nowrap",
                          "padding-left": "0",
                          "max-width": "none",
                        }}
                        title={errText}
                      >
                        {errText}
                      </div>
                    </Show>
                  </div>
                );
              }}
            </For>
          </div>
        </div>
      )}
    </div>
  );
}
