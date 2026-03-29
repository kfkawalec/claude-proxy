import { For, Show, createMemo, createSignal, onMount } from "solid-js";
import type { ModelAgg } from "../lib/hubActivity";
import { config, setConfig, usage } from "../lib/store";
import { api, providerUsage } from "../lib/tauri";
import { t, fmtTemplate } from "../lib/i18n";
import { hubDisplayName } from "../lib/hubDisplayName";
import Button from "./ui/Button";
import {
  aggregateModelsFromActivity,
  activityHasAnyRows,
  formatPeriodRangeShort,
  metadataTotalSpend,
  rollingOneMonthRangeLocal,
  toLocalISODate,
} from "../lib/hubActivity";
import { usageHelperText } from "../lib/usageStyles";

const card: Record<string, string> = {
  display: "flex",
  "flex-direction": "column",
  gap: "10px",
  background: "var(--bg)",
  "border-radius": "8px",
  padding: "10px 8px",
};

/** Maps HTTP 401/403 from fetch_litellm_daily_activity to short copy. */
function friendlyHubDailyActivityError(raw: string): string {
  if (!raw) return raw;
  if (/\b401\b/.test(raw)) {
    return t().usage.hubDailyActivityUnauthorizedError;
  }
  if (
    /\b403\b/.test(raw) &&
    (raw.includes("llm_api_routes") || raw.includes("Virtual key") || raw.includes("/user/daily/activity"))
  ) {
    return t().usage.hubDailyActivityKeyScopeError;
  }
  return raw;
}

function fmt(n: number): string {
  if (n >= 1_000_000) return (n / 1_000_000).toFixed(1) + "M";
  if (n >= 1_000) return (n / 1_000).toFixed(1) + "K";
  return n.toString();
}

/** USD for display (2 decimals). */
function money2(n: number): string {
  return Number.isFinite(n) ? n.toFixed(2) : "0.00";
}

/** Same paths as Rust `json_max_budget`: top-level `max_budget` or `budget.max_budget`. */
function parseMaxBudgetUsd(b: unknown): number | null {
  if (!b || typeof b !== "object") return null;
  const o = b as Record<string, unknown>;
  const read = (v: unknown): number | null => {
    if (typeof v === "number" && Number.isFinite(v)) return v;
    if (typeof v === "string" && v.trim() !== "") {
      const n = Number(v);
      return Number.isFinite(n) ? n : null;
    }
    return null;
  };
  const top = read(o.max_budget);
  if (top != null && top > 0) return top;
  if (top === 0) return 0;
  const nested = o.budget;
  if (nested && typeof nested === "object") {
    const mb = read((nested as Record<string, unknown>).max_budget);
    if (mb != null) return mb;
  }
  const ui = o.user_info;
  if (ui && typeof ui === "object") {
    const mb = read((ui as Record<string, unknown>).max_budget);
    if (mb != null) return mb;
  }
  return top;
}

function ModelNameCell(props: { name: string }) {
  const [tip, setTip] = createSignal<{ x: number; y: number } | null>(null);
  return (
    <>
      <div
        style={{ position: "relative", "min-width": 0, width: "100%" }}
        onMouseEnter={(e) => {
          const r = (e.currentTarget as HTMLDivElement).getBoundingClientRect();
          setTip({ x: r.left, y: r.bottom + 4 });
        }}
        onMouseLeave={() => setTip(null)}
      >
        <span
          style={{ "font-weight": "500", overflow: "hidden", "text-overflow": "ellipsis", "white-space": "nowrap", display: "block" }}
        >
          {props.name}
        </span>
      </div>
      <Show when={tip()}>
        <div
          style={{
            position: "fixed",
            left: `${tip()!.x}px`,
            top: `${tip()!.y}px`,
            "z-index": 9999,
            padding: "6px 8px",
            "max-width": "min(280px, 85vw)",
            "font-size": "11px",
            "font-weight": "500",
            "line-height": "1.35",
            "white-space": "normal",
            "word-break": "break-all",
            background: "var(--bg-card)",
            color: "var(--text-1)",
            border: "0.5px solid var(--border)",
            "border-radius": "6px",
            "box-shadow": "var(--shadow-card)",
            "pointer-events": "none",
          }}
        >
          {props.name}
        </div>
      </Show>
    </>
  );
}

/** Domyślnie pokazuj tyle modeli w „Usage per model”; reszta po „Show more”. */
const MODEL_PREVIEW = 5;

export default function HubUsage() {
  const [loading, setLoading] = createSignal(true);
  const [error, setError] = createSignal<string | null>(null);
  const [configured, setConfigured] = createSignal(true);
  const [budgetInfo, setBudgetInfo] = createSignal<any>(null);
  const [windowPayload, setWindowPayload] = createSignal<any>(null);
  const [modelsExpanded, setModelsExpanded] = createSignal(false);

  const hubName = () => hubDisplayName(config());

  const windowModels = createMemo(() => aggregateModelsFromActivity(windowPayload()));

  const windowSpend = () => metadataTotalSpend(windowPayload());

  const load = async () => {
    setLoading(true);
    setModelsExpanded(false);
    setError(null);
    const cfg = await api.getConfig().catch(() => null as any);
    if (cfg) setConfig(cfg);
    const ok = !!String(cfg?.litellm_api_key ?? "").trim() && !!String(cfg?.litellm_endpoint ?? "").trim();
    setConfigured(ok);
    if (!ok) {
      setLoading(false);
      return;
    }

    const { start: ws, end: we } = rollingOneMonthRangeLocal();
    const startW = toLocalISODate(ws);
    const endW = toLocalISODate(we);

    const budget = await api.fetchBudgetInfo().catch(() => null);
    setBudgetInfo(budget);

    let winJson: any = null;
    let winErr: string | null = null;

    try {
      winJson = await api.fetchLitellmDailyActivity(startW, endW);
    } catch (e) {
      winErr = e instanceof Error ? e.message : String(e);
    }

    setWindowPayload(winJson);
    setError(!winJson ? (winErr ? friendlyHubDailyActivityError(winErr) : "Hub request failed") : null);

    setLoading(false);
  };

  onMount(() => load());

  const handleRefresh = async () => {
    await load();
  };

  /** Prefer LiteLLM / budget `spend` (user-scoped when API provides it); else activity window total. */
  const spend = () => {
    const b = budgetInfo();
    const fromApi = Number(
      b?.spend ?? b?.current_spend ?? b?.usage?.spend ?? b?.user_info?.spend ?? 0
    );
    if (Number.isFinite(fromApi) && fromApi > 0) return fromApi;
    return windowSpend();
  };

  const maxBudgetUsd = () => parseMaxBudgetUsd(budgetInfo());

  const hasRemoteData = () =>
    activityHasAnyRows(windowPayload()) ||
    windowModels().length > 0 ||
    windowSpend() > 0 ||
    Number(budgetInfo()?.spend ?? budgetInfo()?.current_spend ?? 0) > 0;

  const hubRollingRangeText = createMemo(() => {
    const { start, end } = rollingOneMonthRangeLocal();
    return formatPeriodRangeShort(start, end);
  });

  const visibleWindowModels = createMemo((): [string, ModelAgg][] => {
    const all = windowModels();
    if (modelsExpanded() || all.length <= MODEL_PREVIEW) return all;
    return all.slice(0, MODEL_PREVIEW);
  });

  const localHubModels = createMemo(() =>
    Object.entries(providerUsage(usage(), "litellm")?.per_model ?? {}).sort(
      (a: any, b: any) => Number(b?.[1]?.requests ?? 0) - Number(a?.[1]?.requests ?? 0)
    )
  );

  const visibleLocalModels = createMemo((): [string, any][] => {
    const all = localHubModels();
    if (modelsExpanded() || all.length <= MODEL_PREVIEW) return all;
    return all.slice(0, MODEL_PREVIEW);
  });

  const hiddenModelCount = createMemo(() => {
    const remote = windowModels();
    if (remote.length > 0) return Math.max(0, remote.length - MODEL_PREVIEW);
    return Math.max(0, localHubModels().length - MODEL_PREVIEW);
  });

  return (
    <>
      <Show when={!configured()}>
        <div style={card}>
          <div style={usageHelperText}>
            {fmtTemplate(t().usage.hubNotConfigured, { hubName: hubName() })}
          </div>
        </div>
      </Show>

      <Show when={configured()}>
        <Show when={loading()} fallback={null}>
          <div style={{ "font-size": "11px", color: "var(--text-2)", padding: "0 2px" }}>{t().usage.loading}</div>
        </Show>

        <Show when={!loading() && error()}>
          <div style={{ ...card, border: "0.5px solid var(--orange)" }}>
            <div style={{ "font-size": "11px", color: "var(--text-1)", "font-weight": "600" }}>{hubName()} API</div>
            <div style={{ "font-size": "10px", color: "var(--text-2)", "word-break": "break-word" }}>{error()}</div>
            <Button size="sm" onClick={() => load()}>{t().models.refresh}</Button>
          </div>
        </Show>

        <Show when={!loading() && !error() && !hasRemoteData()}>
          <div style={card}>
            <div style={usageHelperText}>{fmtTemplate(t().usage.hubNoData, { hubName: hubName() })}</div>
          </div>
        </Show>

        <Show when={!loading() && !error() && hasRemoteData()}>
          <div style={card}>
            <div style={{ display: "flex", "align-items": "center", "justify-content": "space-between", gap: "8px" }}>
              <div
                style={{
                  display: "flex",
                  "align-items": "baseline",
                  gap: "6px",
                  "flex-wrap": "wrap",
                  "min-width": "0",
                  "font-size": "11px",
                  "line-height": "1.35",
                }}
              >
                <span style={{ "font-weight": "600", color: "var(--text-1)" }}>{t().usage.hubProjectSpendTitle}</span>
                <span style={{ color: "var(--text-2)" }}>{hubRollingRangeText()}</span>
              </div>
              <Button size="sm" onClick={handleRefresh}>{t().models.refresh}</Button>
            </div>
            <div
              style={{
                display: "grid",
                "grid-template-columns": "1fr 1fr",
                gap: "12px",
                "align-items": "start",
              }}
            >
              <div>
                <div style={{ "font-size": "11px", color: "var(--text-2)" }}>{t().usage.totalSpend}</div>
                <div style={{ "font-size": "26px", "font-weight": "700", "letter-spacing": "-0.02em" }}>${money2(spend())}</div>
              </div>
              <div>
                <div style={{ "font-size": "11px", color: "var(--text-2)" }}>{t().usage.maxBudget}</div>
                <div style={{ "font-size": "26px", "font-weight": "700", "letter-spacing": "-0.02em" }}>
                  <Show
                    when={maxBudgetUsd() != null}
                    fallback={<span style={{ color: "var(--text-2)", "font-weight": "600" }}>-</span>}
                  >
                    ${money2(maxBudgetUsd()!)}
                  </Show>
                </div>
              </div>
            </div>
            <div style={usageHelperText}>{t().usage.hubSpendHint}</div>
          </div>

          <div style={card}>
            <div style={{ "font-size": "11px", "font-weight": "600", color: "var(--text-1)", "line-height": "1.35" }}>
              {t().usage.hubPeriodWindowTitle}
            </div>
            <div style={{ "font-size": "11px", "font-weight": "600", "text-transform": "uppercase", color: "var(--text-2)", "letter-spacing": "0.04em", "margin-top": "6px" }}>
              {t().usage.perModel}
            </div>
            <Show
              when={windowModels().length > 0}
              fallback={
                <Show
                  when={localHubModels().length > 0}
                  fallback={<div style={{ "font-size": "11px", color: "var(--text-2)" }}>{t().usage.noHistory}</div>}
                >
                  <For each={visibleLocalModels()}>
                    {([name, v]: any) => (
                      <div style={{ display: "grid", "grid-template-columns": "1fr 46px 60px", gap: "2px", "align-items": "center", "font-size": "11px", border: "0.5px solid var(--border)", background: "var(--bg)", padding: "6px 8px", "border-radius": "7px" }}>
                        <ModelNameCell name={name} />
                        <span style={{ color: "var(--text-2)", "text-align": "right", "font-variant-numeric": "tabular-nums", "white-space": "nowrap" }}>{v.requests ?? 0} req</span>
                        <span style={{ color: "var(--text-2)", "text-align": "right", "font-variant-numeric": "tabular-nums" }}>{fmt(Number(v.input_tokens ?? 0) + Number(v.output_tokens ?? 0))} tok</span>
                      </div>
                    )}
                  </For>
                </Show>
              }
            >
              <For each={visibleWindowModels()}>
                {([name, v]) => (
                  <div style={{ display: "grid", "grid-template-columns": "1fr 46px 52px", gap: "2px", "align-items": "center", "font-size": "11px", border: "0.5px solid var(--border)", background: "var(--bg)", padding: "6px 8px", "border-radius": "7px" }}>
                    <ModelNameCell name={name} />
                    <span style={{ color: "var(--text-2)", "text-align": "right", "font-variant-numeric": "tabular-nums", "white-space": "nowrap" }}>{v.api_requests} req</span>
                    <span style={{ color: "var(--text-2)", "text-align": "right", "font-variant-numeric": "tabular-nums" }}>${money2(v.spend)}</span>
                  </div>
                )}
              </For>
              <Show when={!modelsExpanded() && hiddenModelCount() > 0}>
                <button
                  type="button"
                  onClick={() => setModelsExpanded(true)}
                  style={{
                    "font-size": "11px",
                    color: "var(--accent)",
                    background: "transparent",
                    border: "none",
                    cursor: "pointer",
                    padding: "4px 0",
                    "text-align": "left",
                    "font-weight": "600",
                  }}
                >
                  {t().usage.showMoreModels.replace("{{count}}", String(hiddenModelCount()))}
                </button>
              </Show>
            </Show>
          </div>
        </Show>
      </Show>
    </>
  );
}
