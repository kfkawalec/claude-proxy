import { JSX } from "solid-js";

interface Props {
  children: JSX.Element;
  onClick?: () => void;
  variant?: "primary" | "secondary" | "danger";
  size?: "sm" | "md";
  disabled?: boolean;
  /** Pełna szerokość kontenera (np. CTA w panelu). */
  fullWidth?: boolean;
}

export default function Button(props: Props) {
  const isPrimary = () => props.variant === "primary";
  const isDanger  = () => props.variant === "danger";
  const isSm      = () => props.size === "sm";

  const bg = () =>
    isPrimary() ? "var(--accent)" :
    isDanger()  ? "var(--red)" :
    "var(--bg-seg)";

  const color = () =>
    isPrimary() || isDanger() ? "#fff" : "var(--text-1)";

  return (
    <button
      disabled={props.disabled}
      onClick={props.onClick}
      style={{
        display: "inline-flex",
        "align-items": "center",
        "justify-content": "center",
        width: props.fullWidth ? "100%" : undefined,
        "box-sizing": "border-box",
        height: isSm() ? "24px" : "28px",
        padding: isSm() ? "0 10px" : "0 12px",
        "border-radius": "7px",
        border: "none",
        "font-size": isSm() ? "11px" : "12px",
        "font-weight": "500",
        "letter-spacing": "-0.01em",
        color: color(),
        background: bg(),
        opacity: props.disabled ? "0.4" : "1",
        cursor: "default",
        transition: "opacity 0.15s",
        "white-space": "nowrap",
        "flex-shrink": "0",
      }}
      onMouseEnter={(e) => { if (!props.disabled) e.currentTarget.style.opacity = "0.82"; }}
      onMouseLeave={(e) => { if (!props.disabled) e.currentTarget.style.opacity = "1"; }}
    >
      {props.children}
    </button>
  );
}
