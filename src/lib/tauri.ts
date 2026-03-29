import { invoke } from "@tauri-apps/api/core";

export interface AppConfig {
  provider: string;
  port: number;
  litellm_api_key: string;
  litellm_endpoint: string;
  litellm_display_name: string;
  model_overrides: Record<string, string>;
}

export interface ProviderUsage {
  input_tokens: number;
  output_tokens: number;
  requests: number;
  per_model: Record<string, { input_tokens: number; output_tokens: number; requests: number }>;
}

export interface UsageData {
  by_provider: Record<string, ProviderUsage>;
}

export function providerUsage(
  u: UsageData | null | undefined,
  id: string,
): ProviderUsage | undefined {
  return u?.by_provider?.[id];
}

export type ProxyStatus = "Running" | "Stopped" | { Error: string };

export interface ProxyActivityEntry {
  ts_ms: number;
  provider: string;
  method: string;
  path: string;
  status: number;
  model: string | null;
  /** Czas od startu obsługi żądania do końca odpowiedzi (ms). */
  duration_ms: number;
  input_tokens: number;
  output_tokens: number;
  /** Krótki komunikat z body, gdy status nie jest 2xx. */
  error_detail?: string | null;
}

export interface ClaudeInstallStatus {
  installed: boolean;
  settings_path: string;
  current_base_url: string | null;
}

export const PROXY_PORT = 3456;

export const api = {
  getConfig: () => invoke<AppConfig>("get_config"),
  saveSettings: (config: AppConfig) => invoke<void>("save_settings", { config }),
  setProvider: (provider: string) => invoke<void>("set_provider", { provider }),
  getProxyStatus: () => invoke<ProxyStatus>("get_proxy_status"),
  getUsage: () => invoke<UsageData>("get_usage"),
  resetUsage: () => invoke<void>("reset_usage"),
  fetchModels: () => invoke<any[]>("fetch_models"),
  fetchBudgetInfo: () => invoke<any>("fetch_budget_info"),
  fetchLitellmDailyActivity: (start_date: string, end_date: string) =>
    invoke<any>("fetch_litellm_daily_activity", { startDate: start_date, endDate: end_date }),
  getClaudeInstallStatus: () => invoke<ClaudeInstallStatus>("get_claude_install_status"),
  installClaudeProxySettings: () => invoke<ClaudeInstallStatus>("install_claude_proxy_settings"),
  uninstallClaudeProxySettings: () => invoke<ClaudeInstallStatus>("uninstall_claude_proxy_settings"),
  checkClaudeAuth: () => invoke<boolean>("check_claude_auth"),
  claudeLogin: () => invoke<void>("claude_login"),
  openUrl: (url: string) => invoke<void>("open_url", { url }),
  getProxyActivity: () => invoke<ProxyActivityEntry[]>("get_proxy_activity"),
  fetchClaudeRateLimits: () => invoke<{
    has_auth: boolean;
    five_hour_utilization: number | null;
    five_hour_resets_at: string | null;
    seven_day_utilization: number | null;
    seven_day_resets_at: string | null;
  }>("fetch_claude_rate_limits"),
};
