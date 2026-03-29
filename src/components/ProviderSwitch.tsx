import { createMemo } from "solid-js";
import { config, setConfig, showToast } from "../lib/store";
import { api } from "../lib/tauri";
import { t } from "../lib/i18n";
import { hubDisplayName } from "../lib/hubDisplayName";
import SegmentedControl from "./ui/SegmentedControl";

export default function ProviderSwitch(props: { compact?: boolean }) {
  const litellmLabel = createMemo(() => hubDisplayName(config()));

  const handleChange = async (value: string) => {
    await api.setProvider(value);
    const updated = await api.getConfig();
    setConfig(updated);
    const name = value === "litellm" ? litellmLabel() : "Claude";
    showToast(`${t().toast.providerSet} ${name}`);
  };

  return (
    <div style={{ padding: props.compact ? "0 0 6px 0" : "10px 12px 8px", "flex-shrink": "0" }}>
      <SegmentedControl
        large
        options={[
          { value: "claude", label: "Claude" },
          { value: "litellm", label: litellmLabel() },
        ]}
        value={config()?.provider ?? "claude"}
        onChange={handleChange}
      />
    </div>
  );
}
