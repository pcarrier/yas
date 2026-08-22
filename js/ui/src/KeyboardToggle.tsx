import type { JSX } from "solid-js";
import { t } from "./i18n";
import { TapButton } from "./TapButton";

/** Focus the IME from the trusted touch gesture, which Safari may not turn
 * into a click. Cancel touch defaults so they cannot blur the input again. */
export function KeyboardToggle(props: {
  open?: boolean;
  onToggle?: () => void;
  style: JSX.CSSProperties;
}) {
  return (
    <TapButton
      onActivate={() => props.onToggle?.()}
      style={{
        ...props.style,
        opacity: props.open ? 1 : 0.5,
        "touch-action": "manipulation",
      }}
      title={
        props.open ? t("statusbar.hideKeyboard") : t("statusbar.showKeyboard")
      }
    >
      <svg
        width="1em"
        height="1em"
        viewBox="0 0 16 16"
        fill="none"
        stroke="currentColor"
        stroke-width="1.2"
        stroke-linecap="round"
        stroke-linejoin="round"
        style={{ display: "block", "pointer-events": "none" }}
      >
        <rect x="1" y="3" width="14" height="10" rx="1.5" />
        <line x1="4" y1="6" x2="5" y2="6" />
        <line x1="7.5" y1="6" x2="8.5" y2="6" />
        <line x1="11" y1="6" x2="12" y2="6" />
        <line x1="4" y1="9" x2="5" y2="9" />
        <line x1="11" y1="9" x2="12" y2="9" />
        <line x1="7" y1="9" x2="9" y2="9" />
      </svg>
    </TapButton>
  );
}
