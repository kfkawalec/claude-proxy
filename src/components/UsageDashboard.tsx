import { ask } from "@tauri-apps/plugin-dialog";
import { config, usage, setUsage } from "../lib/store";
import { hubDisplayName } from "../lib/hubDisplayName";
import { api, providerUsage } from "../lib/tauri";
import { onMount, Show, createSignal, createEffect, createMemo } from "solid-js";
import { t } from "../lib/i18n";
import { usageHelperText } from "../lib/usageStyles";
import Button from "./ui/Button";
import HubUsage from "./HubUsage";

/** Blok wewnątrz panelu — tylko tło, bez drugiej ramki. */
const card: Record<string, string> = {
  display: "flex",
  "flex-direction": "column",
  gap: "10px",
  background: "var(--bg)",
  "border-radius": "8px",
  padding: "10px 8px",
  "min-width": "0",
  "box-sizing": "border-box",
};

function progressBarPct(percent: number) {
  return (
    <div
      style={{
        width: "100%",
        height: "4px",
        background: "var(--bg)",
        border: "0.5px solid var(--border)",
        "border-radius": "999px",
        overflow: "hidden",
        "margin-top": "6px",
        "box-sizing": "border-box",
      }}
    >
      <div
        style={{
          width: `${Math.max(0, Math.min(100, percent))}%`,
          height: "100%",
          background: "var(--accent)",
          transition: "width 0.2s",
          "border-radius": "999px",
        }}
      />
    </div>
  );
}

function formatResetDuration(iso: string | null): string {
  if (!iso) return "";
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return iso;
  const diff = d.getTime() - Date.now();
  if (diff <= 0) return "now";
  const mins = Math.round(diff / 60_000);
  if (mins < 60) return `${mins} min`;
  const hrs = Math.floor(mins / 60);
  const remMins = mins % 60;
  return `${hrs} hr ${remMins} min`;
}

function formatResetDay(iso: string | null): string {
  if (!iso) return "";
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return iso;
  const day = d.toLocaleDateString(undefined, { weekday: "short" });
  const time = d.toLocaleTimeString(undefined, { hour: "numeric", minute: "2-digit" });
  return `${day} ${time}`;
}

function relativeTime(date: Date | null): string {
  if (!date) return "";
  const diff = Date.now() - date.getTime();
  const secs = Math.round(diff / 1000);
  if (secs < 10) return "just now";
  if (secs < 60) return `${secs} seconds ago`;
  const mins = Math.round(secs / 60);
  if (mins === 1) return "1 minute ago";
  return `${mins} minutes ago`;
}

function utilizationToPercent(value: number | null | undefined): number {
  const v = Number(value ?? 0);
  if (!Number.isFinite(v)) return 0;
  const pct = v <= 1 ? v * 100 : v;
  return Math.max(0, Math.min(100, Math.round(pct)));
}

function fmtTok(n: number) {
  return n.toLocaleString();
}

const statsSectionLabel: Record<string, string> = {
  "font-size": "10px",
  "font-weight": "600",
  color: "var(--text-2)",
  "text-transform": "uppercase",
  "letter-spacing": "0.06em",
};

/** Jedna karta: sumy lokalne dla Claude i huba obok siebie (nagłówek sekcji na zewnątrz). */
function LocalProviderTotalsCombined() {
  const claude = () => providerUsage(usage(), "claude");
  const hub = () => providerUsage(usage(), "litellm");
  const hubLabel = () => hubDisplayName(config());
  const cellNum: Record<string, string> = {
    "font-weight": "600",
    "font-variant-numeric": "tabular-nums",
    "text-align": "right",
  };
  const colHead: Record<string, string> = {
    color: "var(--text-2)",
    "font-size": "10px",
    "font-weight": "600",
    "text-align": "right",
  };
  return (
    <div style={card}>
      <div
        style={{
          display: "grid",
          "grid-template-columns": "minmax(76px,1fr) 1fr 1fr 1fr",
          gap: "6px 8px",
          "font-size": "11px",
          "align-items": "baseline",
        }}
      >
        <div />
        <div style={colHead}>{t().usage.requests}</div>
        <div style={colHead}>{t().usage.inTokens}</div>
        <div style={colHead}>{t().usage.outTokens}</div>

        <div style={{ color: "var(--text-2)", "font-weight": "600" }}>Claude</div>
        <div style={cellNum}>{fmtTok(claude()?.requests ?? 0)}</div>
        <div style={cellNum}>{fmtTok(claude()?.input_tokens ?? 0)}</div>
        <div style={cellNum}>{fmtTok(claude()?.output_tokens ?? 0)}</div>

        <div
          style={{
            color: "var(--text-2)",
            "font-weight": "600",
            overflow: "hidden",
            "text-overflow": "ellipsis",
            "white-space": "nowrap",
          }}
          title={hubLabel()}
        >
          {hubLabel()}
        </div>
        <div style={cellNum}>{fmtTok(hub()?.requests ?? 0)}</div>
        <div style={cellNum}>{fmtTok(hub()?.input_tokens ?? 0)}</div>
        <div style={cellNum}>{fmtTok(hub()?.output_tokens ?? 0)}</div>
      </div>
    </div>
  );
}

/** Fetches plan usage from GET /api/oauth/usage. Auto-refreshes every 5 requests. */
function ClaudeRateLimits(props: { requestCount: number; onRefresh: () => Promise<void> }) {
  const [limits, setLimits] = createSignal<any>(null);
  const [loading, setLoading] = createSignal(false);
  const [updatedAt, setUpdatedAt] = createSignal<Date | null>(null);
  const [lastAutoRefreshAt, setLastAutoRefreshAt] = createSignal(0);

  const fetchLimits = async () => {
    setLoading(true);
    const data = await api.fetchClaudeRateLimits().catch(() => null);
    setLimits(data);
    setUpdatedAt(new Date());
    setLoading(false);
  };

  onMount(fetchLimits);

  createEffect(() => {
    const count = Number(props.requestCount ?? 0);
    const last = lastAutoRefreshAt();
    if (count > 0 && count % 5 === 0 && count !== last) {
      setLastAutoRefreshAt(count);
      void fetchLimits();
    }
  });

  const data = createMemo(() => limits() ?? { has_auth: false });
  const hasData = () => data().five_hour_utilization != null || data().seven_day_utilization != null;

  const sessionPct = () => utilizationToPercent(data().five_hour_utilization);
  const weeklyPct = () => utilizationToPercent(data().seven_day_utilization);

  return (
    <div style={card}>
      <div style={{ display: "flex", "align-items": "center", "justify-content": "space-between", gap: "8px", "min-width": "0" }}>
        <div
          style={{
            display: "flex",
            "align-items": "baseline",
            gap: "6px",
            "flex-wrap": "wrap",
            "min-width": "0",
            "font-size": "11px",
            "line-height": "1.35",
            flex: "1 1 auto",
          }}
        >
          <span style={{ "font-weight": "600", color: "var(--text-1)" }}>{t().usage.planUsageLimits}</span>
          <Show when={updatedAt()}>
            <span style={{ color: "var(--text-2)" }}>
              {t().usage.lastUpdated}: {relativeTime(updatedAt())}
            </span>
          </Show>
        </div>
        <Button size="sm" onClick={async () => { await props.onRefresh(); await fetchLimits(); }} disabled={loading()}>
          {loading() ? "..." : t().models.refresh}
        </Button>
      </div>

      <Show when={!loading() && (!data().has_auth || !hasData())}>
        <div style={usageHelperText}>{t().usage.noLimitsYet}</div>
      </Show>

      <Show when={hasData()}>
        <div
          style={{
            display: "flex",
            "align-items": "center",
            gap: "8px",
            "min-width": "0",
            width: "100%",
          }}
        >
          <div style={{ "font-size": "12px", "font-weight": "600", "flex-shrink": "0" }}>{t().usage.currentSession}</div>
          <div
            style={{
              "font-size": "11px",
              color: "var(--text-2)",
              "min-width": "0",
              flex: "1 1 auto",
              overflow: "hidden",
              "text-overflow": "ellipsis",
              "white-space": "nowrap",
              "text-align": "left",
            }}
            title={data().five_hour_resets_at ? `Resets in ${formatResetDuration(data().five_hour_resets_at)}` : ""}
          >
            {data().five_hour_resets_at ? `Resets in ${formatResetDuration(data().five_hour_resets_at)}` : ""}
          </div>
          <div style={{ "font-size": "12px", color: "var(--text-2)", "flex-shrink": "0", "white-space": "nowrap" }}>
            {sessionPct()}% {t().usage.used}
          </div>
        </div>
        {progressBarPct(sessionPct())}

        <div style={{ height: "0.5px", background: "var(--border)", margin: "2px 0" }} />

        <div style={{ "font-size": "11px", "font-weight": "600", color: "var(--text-1)", "letter-spacing": "-0.01em" }}>
          {t().usage.weekly}
        </div>
        <div
          style={{
            display: "flex",
            "align-items": "center",
            gap: "8px",
            "min-width": "0",
            width: "100%",
          }}
        >
          <div style={{ "font-size": "12px", "font-weight": "600", "flex-shrink": "0" }}>{t().usage.allModels}</div>
          <div
            style={{
              "font-size": "11px",
              color: "var(--text-2)",
              "min-width": "0",
              flex: "1 1 auto",
              overflow: "hidden",
              "text-overflow": "ellipsis",
              "white-space": "nowrap",
              "text-align": "left",
            }}
            title={data().seven_day_resets_at ? `Resets ${formatResetDay(data().seven_day_resets_at)}` : ""}
          >
            {data().seven_day_resets_at ? `Resets ${formatResetDay(data().seven_day_resets_at)}` : ""}
          </div>
          <div style={{ "font-size": "12px", color: "var(--text-2)", "flex-shrink": "0", "white-space": "nowrap" }}>
            {weeklyPct()}% {t().usage.used}
          </div>
        </div>
        {progressBarPct(weeklyPct())}
      </Show>

      <Show when={data().has_auth && !loading()}>
        <a
          href="#"
          onClick={(e) => { e.preventDefault(); api.openUrl("https://claude.ai/settings/usage"); }}
          style={{ "font-size": "11px", color: "var(--accent)", "text-decoration": "none", cursor: "pointer", "margin-top": "2px" }}
        >
          {t().settings.limitsLink} →
        </a>
      </Show>
    </div>
  );
}

export default function UsageDashboard() {
  const [resetBusy, setResetBusy] = createSignal(false);

  const loadUsage = async () => {
    setUsage(await api.getUsage().catch(() => usage() as any));
  };

  onMount(loadUsage);

  const claudeReq = () => providerUsage(usage(), "claude")?.requests ?? 0;

  const handleResetLocalTotals = async () => {
    const ok = await ask(t().usage.resetLocalTotalsConfirm, {
      title: "Claude Proxy",
      kind: "warning",
    });
    if (!ok) return;
    setResetBusy(true);
    try {
      await api.resetUsage();
      await loadUsage();
    } finally {
      setResetBusy(false);
    }
  };

  return (
    <div style={{ height: "100%", display: "flex", "flex-direction": "column", overflow: "hidden", "min-height": "0" }}>
      <div style={{ flex: "1", "min-height": "0", overflow: "auto", padding: "10px 8px", display: "flex", "flex-direction": "column", gap: "10px" }}>
        <div style={{ display: "flex", "flex-direction": "column", gap: "6px" }}>
          <div
            style={{
              display: "flex",
              "align-items": "center",
              "justify-content": "space-between",
              gap: "8px",
              "min-width": "0",
            }}
          >
            <div style={statsSectionLabel}>{t().usage.sectionViaProxy}</div>
            <Button size="sm" variant="secondary" disabled={resetBusy()} onClick={handleResetLocalTotals}>
              {resetBusy() ? t().usage.loading : t().usage.reset}
            </Button>
          </div>
          <div style={{ ...usageHelperText, "white-space": "nowrap", overflow: "hidden", "text-overflow": "ellipsis" }}>
            {t().usage.localProxyTotalsHint}
          </div>
          <LocalProviderTotalsCombined />
        </div>
        <div style={{ display: "flex", "flex-direction": "column", gap: "6px" }}>
          <div style={statsSectionLabel}>{t().usage.sectionClaude}</div>
          <ClaudeRateLimits requestCount={claudeReq()} onRefresh={loadUsage} />
        </div>
        <div style={{ display: "flex", "flex-direction": "column", gap: "6px" }}>
          <div style={statsSectionLabel} title={hubDisplayName(config())}>
            {hubDisplayName(config())}
          </div>
          <HubUsage />
        </div>
      </div>
    </div>
  );
}
