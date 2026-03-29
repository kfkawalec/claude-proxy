/** Calendar date in local timezone (YYYY-MM-DD). Never use toISOString() for API ranges - UTC shifts the day. */
export function toLocalISODate(d: Date): string {
  const y = d.getFullYear();
  const m = String(d.getMonth() + 1).padStart(2, "0");
  const day = String(d.getDate()).padStart(2, "0");
  return `${y}-${m}-${day}`;
}

export function monthRangeLocal(anchor: Date): { start: Date; end: Date } {
  const y = anchor.getFullYear();
  const m = anchor.getMonth();
  const start = new Date(y, m, 1);
  const end = new Date(y, m + 1, 0);
  return { start, end };
}

export function rollingDaysRangeLocal(days: number): { start: Date; end: Date } {
  const end = new Date();
  const start = new Date();
  start.setHours(0, 0, 0, 0);
  end.setHours(23, 59, 59, 999);
  start.setDate(start.getDate() - Math.max(1, days) + 1);
  return { start, end };
}

/** Od tego samego dnia miesiąca (miesiąc wstecz) do końca dziś, lokalna strefa. Np. 9 kwi → zakres 9 mar – 9 kwi. */
export function rollingOneMonthRangeLocal(): { start: Date; end: Date } {
  const now = new Date();
  const end = new Date(now.getFullYear(), now.getMonth(), now.getDate(), 23, 59, 59, 999);
  const start = new Date(now.getFullYear(), now.getMonth(), now.getDate(), 0, 0, 0, 0);
  start.setMonth(start.getMonth() - 1);
  return { start, end };
}

/** Shared style for hub usage headers, e.g. "Mar 2 – Mar 31, 2026" (locale month names). */
export function formatPeriodRangeShort(start: Date, end: Date): string {
  const mo: Intl.DateTimeFormatOptions = { month: "short" };
  const sm = start.toLocaleDateString(undefined, mo);
  const em = end.toLocaleDateString(undefined, mo);
  const sd = start.getDate();
  const ed = end.getDate();
  const sy = start.getFullYear();
  const ey = end.getFullYear();
  if (sy === ey) {
    return `${sm} ${sd} – ${em} ${ed}, ${ey}`;
  }
  return `${sm} ${sd}, ${sy} – ${em} ${ed}, ${ey}`;
}

/** LiteLLM-style: model row may be flat or under `.metrics`. */
export function extractHubModelMetrics(v: unknown): {
  spend: number;
  total_tokens: number;
  api_requests: number;
} {
  const o = v as Record<string, unknown>;
  const m = (o?.metrics ?? o) as Record<string, unknown>;
  return {
    spend: Number(m?.spend ?? 0),
    total_tokens: Number(m?.total_tokens ?? 0),
    api_requests: Number(m?.api_requests ?? 0),
  };
}

export type ModelAgg = { spend: number; total_tokens: number; api_requests: number };

export function aggregateModelsFromActivity(payload: unknown): [string, ModelAgg][] {
  const rows = (payload as { results?: unknown[] })?.results ?? [];
  const agg: Record<string, ModelAgg> = {};
  for (const day of rows) {
    const models = (day as { breakdown?: { models?: Record<string, unknown> } })?.breakdown?.models ?? {};
    for (const [name, v] of Object.entries(models)) {
      if (!agg[name]) agg[name] = { spend: 0, total_tokens: 0, api_requests: 0 };
      const m = extractHubModelMetrics(v);
      agg[name].spend += m.spend;
      agg[name].total_tokens += m.total_tokens;
      agg[name].api_requests += m.api_requests;
    }
  }
  return Object.entries(agg).sort((a, b) => b[1].spend - a[1].spend);
}

export function metadataTotalSpend(payload: unknown): number {
  const v = (payload as { metadata?: { total_spend?: number } })?.metadata?.total_spend;
  return Number(v ?? 0);
}

export function activityHasAnyRows(payload: unknown): boolean {
  const results = (payload as { results?: unknown[] })?.results;
  if (!Array.isArray(results) || results.length === 0) return false;
  return results.some((day: any) => {
    const spend = Number(day?.metrics?.spend ?? 0);
    const models = day?.breakdown?.models ?? {};
    return spend > 0 || Object.keys(models).length > 0;
  });
}
