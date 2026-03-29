import { createSignal } from "solid-js";

interface Props {
  value: string;
  onInput: (value: string) => void;
  placeholder?: string;
  type?: "text" | "password" | "number";
  label?: string;
  showToggle?: boolean;
}

export default function Input(props: Props) {
  const [visible, setVisible] = createSignal(false);

  const effectiveType = () => {
    if (props.type === "password" && props.showToggle) {
      return visible() ? "text" : "password";
    }
    return props.type ?? "text";
  };

  return (
    <div style={{ display: "flex", "flex-direction": "column", gap: "5px" }}>
      {props.label && (
        <label style={{
          "font-size": "11px",
          "font-weight": "500",
          color: "var(--text-2)",
          "letter-spacing": "0.02em",
          "text-transform": "uppercase",
        }}>
          {props.label}
        </label>
      )}
      <div style={{ position: "relative" }}>
        <input
          type={effectiveType()}
          value={props.value}
          onInput={(e) => props.onInput(e.currentTarget.value)}
          placeholder={props.placeholder}
          style={{
            width: "100%",
            height: "28px",
            padding: props.showToggle ? "0 32px 0 10px" : "0 10px",
            "border-radius": "7px",
            border: "0.5px solid var(--border-input)",
            background: "var(--bg-input)",
            "font-size": "12px",
            color: "var(--text-1)",
            outline: "none",
            transition: "border-color 0.15s, box-shadow 0.15s",
            "font-family": "inherit",
            "box-sizing": "border-box",
          }}
          onFocus={(e) => {
            e.currentTarget.style.borderColor = "var(--accent)";
            e.currentTarget.style.boxShadow = "0 0 0 2px rgba(0,122,255,0.2)";
          }}
          onBlur={(e) => {
            e.currentTarget.style.borderColor = "var(--border-input)";
            e.currentTarget.style.boxShadow = "none";
          }}
        />
        {props.showToggle && props.type === "password" && (
          <button
            type="button"
            onClick={() => setVisible((v) => !v)}
            style={{
              position: "absolute",
              right: "8px",
              top: "50%",
              transform: "translateY(-50%)",
              background: "none",
              border: "none",
              padding: "0",
              cursor: "pointer",
              color: "var(--text-3)",
              display: "flex",
              "align-items": "center",
              "justify-content": "center",
              transition: "color 0.15s",
            }}
            onMouseEnter={(e) => e.currentTarget.style.color = "var(--text-1)"}
            onMouseLeave={(e) => e.currentTarget.style.color = "var(--text-3)"}
            title={visible() ? "Ukryj klucz" : "Pokaż klucz"}
          >
            {visible() ? (
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                <path d="M17.94 17.94A10.07 10.07 0 0 1 12 20c-7 0-11-8-11-8a18.45 18.45 0 0 1 5.06-5.94"/>
                <path d="M9.9 4.24A9.12 9.12 0 0 1 12 4c7 0 11 8 11 8a18.5 18.5 0 0 1-2.16 3.19"/>
                <line x1="1" y1="1" x2="23" y2="23"/>
              </svg>
            ) : (
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                <path d="M1 12s4-8 11-8 11 8 11 8-4 8-11 8-11-8-11-8z"/>
                <circle cx="12" cy="12" r="3"/>
              </svg>
            )}
          </button>
        )}
      </div>
    </div>
  );
}
