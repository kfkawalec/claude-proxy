import type { AppConfig } from "./tauri";

export function hubDisplayName(cfg: AppConfig | null | undefined): string {
  const n = String(cfg?.litellm_display_name ?? "").trim();
  if (n) return n;
  return "Hub";
}
