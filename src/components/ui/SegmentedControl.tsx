import { For } from "solid-js";

interface Props {
  options: { value: string; label: string }[];
  value: string;
  onChange: (value: string) => void;
  large?: boolean;
}

export default function SegmentedControl(props: Props) {
  const h = () => props.large ? "36px" : "28px";
  const fs = () => props.large ? "13px" : "12px";
  const fw = () => props.large ? "500" : "450";

  return (
    <div style={{
      display: "flex",
      background: "var(--bg-seg)",
      "border-radius": "9px",
      padding: "3px",
      gap: "2px",
      height: h(),
    }}>
      <For each={props.options}>
        {(option) => {
          const active = () => props.value === option.value;
          return (
            <button
              onClick={() => props.onChange(option.value)}
              style={{
                flex: "1",
                "border-radius": "7px",
                border: "none",
                "font-size": fs(),
                "font-weight": active() ? "590" : fw(),
                "letter-spacing": "-0.01em",
                color: active() ? "var(--text-1)" : "var(--text-2)",
                background: active() ? "var(--bg-seg-pill)" : "transparent",
                "box-shadow": active() ? "var(--shadow-pill)" : "none",
                transition: "color 0.15s, background 0.15s, box-shadow 0.15s",
                cursor: "default",
                padding: "0 10px",
                "white-space": "nowrap",
              }}
            >
              {option.label}
            </button>
          );
        }}
      </For>
    </div>
  );
}
