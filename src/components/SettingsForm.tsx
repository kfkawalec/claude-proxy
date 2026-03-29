import { setConfig } from "../lib/store";
import { api, type AppConfig } from "../lib/tauri";
import { createSignal, onMount } from "solid-js";
import { t } from "../lib/i18n";
import Input from "./ui/Input";
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

/** OpenAI-compatible endpoint + Save. Aktualizacje aplikacji: `AppUpdatesSection` w TrayPopover. Mapowanie: ModelBrowser. */
export default function SettingsForm() {
  const [form, setForm] = createSignal<AppConfig>({
    provider: "claude",
    port: 3456,
    litellm_api_key: "",
    litellm_endpoint: "",
    litellm_display_name: "",
    model_overrides: {},
  });
  const [saved, setSaved] = createSignal(false);

  onMount(async () => {
    const cfg = await api.getConfig();
    setForm(cfg);
    setConfig(cfg);
  });

  const update = (key: keyof AppConfig, val: any) => {
    setForm((p) => ({ ...p, [key]: val }));
    setSaved(false);
  };

  const handleSave = async () => {
    const latest = await api.getConfig().catch(() => null);
    const f = form();
    const merged: AppConfig = {
      ...(latest ?? f),
      litellm_api_key: f.litellm_api_key,
      litellm_endpoint: f.litellm_endpoint,
      litellm_display_name: f.litellm_display_name,
      provider: f.provider,
      port: f.port,
      model_overrides: latest?.model_overrides ?? f.model_overrides,
    };
    await api.saveSettings(merged);
    setForm(merged);
    setConfig(merged);
    setSaved(true);
    setTimeout(() => setSaved(false), 2000);
  };

  return (
    <div style={{ display: "flex", "flex-direction": "column", gap: "14px", padding: "0" }}>
      <div style={{ display: "flex", "flex-direction": "column", gap: "6px" }}>
        <div style={sectionLabel}>{t().settings.openAiCompatibleEndpoint}</div>
        <div style={card}>
          <Input
            label={t().settings.hubDisplayName}
            value={form().litellm_display_name}
            onInput={(v) => update("litellm_display_name", v)}
            placeholder="My Hub"
          />
          <Input
            label={t().settings.endpoint}
            value={form().litellm_endpoint}
            onInput={(v) => update("litellm_endpoint", v)}
            placeholder="https://hub.example.com"
          />
          <Input
            label={t().settings.apiKey}
            type="password"
            showToggle
            value={form().litellm_api_key}
            onInput={(v) => update("litellm_api_key", v)}
            placeholder="sk-…"
          />
          <div
            style={{
              display: "flex",
              "flex-direction": "row",
              "justify-content": "flex-end",
              "align-items": "center",
              gap: "8px",
              "flex-wrap": "wrap",
              width: "100%",
              "padding-top": "2px",
            }}
          >
            {saved() && (
              <span style={{ "font-size": "11px", color: "var(--green)", "font-weight": "500", "margin-right": "auto" }}>{t().settings.saved}</span>
            )}
            <Button variant="primary" onClick={handleSave}>{t().settings.save}</Button>
          </div>
        </div>
      </div>
    </div>
  );
}
