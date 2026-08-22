import type { JSX } from "solid-js";
import type { YasSession, YasSurface, TerminalPalette } from "@yas-run/core";

/** Display name for a session (title/command, no ptyId — use sessionPrefix() for that). */
export function sessionName(s: YasSession): string {
  const hasTitle = s.title != null && s.title.length > 0;
  const hasCommand = s.command != null && s.command.length > 0;
  if (hasTitle && hasCommand && s.title !== s.command) {
    return `${s.title} \u00B7 ${s.command}`;
  }
  if (hasTitle) return s.title!;
  if (hasCommand) return s.command!;
  return `${s.ptyId}`;
}

/** Gray prefix shown before sessionName(): "remote:ptyId" or just "ptyId". */
export function sessionPrefix(
  s: YasSession,
  connectionLabel?: string | null,
): string {
  return connectionLabel ? `${connectionLabel}:${s.ptyId}` : `${s.ptyId}`;
}

/** Display name for a surface (title/appId, no surfaceId prefix). */
export function surfaceName(s: YasSurface): string {
  return s.title || s.appId || `Surface ${s.surfaceId}`;
}

/** Gray prefix shown before surfaceName(): "remote:Sid" or just "Sid". */
export function surfacePrefix(
  s: YasSurface,
  connectionLabel?: string | null,
): string {
  return connectionLabel
    ? `${connectionLabel}:S${s.surfaceId}`
    : `S${s.surfaceId}`;
}

export interface Theme {
  bg: string;
  fg: string;
  dimFg: string;
  panelBg: string;
  solidPanelBg: string;
  inputBg: string;
  solidInputBg: string;
  border: string;
  subtleBorder: string;
  hoverBg: string;
  selectedBg: string;
  accent: string;
  error: string;
  errorText: string;
  success: string;
  warning: string;
}

export interface UIScale {
  xs: number;
  sm: number;
  md: number;
  lg: number;
  xl: number;
  tightGap: number;
  gap: number;
  panelPadding: number;
  controlY: number;
  controlX: number;
  icon: number;
}

export function uiScale(baseFontSize: number): UIScale {
  const base = Math.max(10, Math.round(baseFontSize || 13));
  const max = Math.round(base * 1.25);
  const scaled = (multiplier: number, floor: number) =>
    Math.max(floor, Math.min(max, Math.round(base * multiplier)));

  return {
    xs: scaled(0.78, 9),
    sm: scaled(0.88, 10),
    md: scaled(1, base),
    lg: scaled(1.08, base),
    xl: scaled(1.18, base),
    tightGap: Math.max(4, Math.round(base * 0.3)),
    gap: Math.max(6, Math.round(base * 0.45)),
    panelPadding: Math.max(8, Math.round(base * 0.6)),
    controlY: Math.max(3, Math.round(base * 0.32)),
    controlX: Math.max(6, Math.round(base * 0.55)),
    icon: Math.max(44, Math.round(base * 3.7)),
  };
}

/** Shared dimensions for the workspace's top and bottom bars. */
export function workspaceBarSizing(scale: UIScale, isMobileTouch = false) {
  const touchScale = isMobileTouch ? 2 : 1;
  const iconSize = Math.round(scale.md * touchScale);
  return {
    touchScale,
    fontSize: scale.md,
    iconSize,
    buttonWidth: Math.ceil(scale.md * 1.75 * touchScale),
    height: Math.max(scale.md + scale.controlY * 3, iconSize + scale.controlY),
  };
}

export function workspaceBarStyle(
  scale: UIScale,
  isMobileTouch = false,
): JSX.CSSProperties {
  const bar = workspaceBarSizing(scale, isMobileTouch);
  return {
    "box-sizing": "border-box",
    height: `${bar.height}px`,
    "min-height": `${bar.height}px`,
    "flex-shrink": 0,
    "font-size": `${bar.fontSize}px`,
    "line-height": 1,
    "--yas-bar-icon-size": `${bar.iconSize}px`,
    "--yas-bar-button-width": `${bar.buttonWidth}px`,
  };
}

/**
 * What the chrome looks like before a palette is known.
 *
 * Solid throughout, for the same reason the palette-derived theme is: a
 * translucent panel takes its colour from whatever it happens to be over, and
 * what it took was grey. These are the standard xterm tones the default
 * palette carries anyway, so the loading screen and the running workspace
 * agree.
 */
export const darkTheme: Theme = {
  bg: "#1a1a1a",
  fg: "#e0e0e0",
  dimFg: "#808080",
  panelBg: "#000000",
  solidPanelBg: "#000000",
  inputBg: "#1a1a1a",
  solidInputBg: "#1a1a1a",
  border: "#808080",
  subtleBorder: "#1a1a1a",
  hoverBg: "#1a1a1a",
  selectedBg: "#808080",
  accent: "#58f",
  error: "#a44",
  errorText: "#f55",
  success: "#4a4",
  warning: "#da3",
};

export const lightTheme: Theme = {
  bg: "#f5f5f5",
  fg: "#333333",
  dimFg: "#808080",
  panelBg: "#ffffff",
  solidPanelBg: "#ffffff",
  inputBg: "#f5f5f5",
  solidInputBg: "#f5f5f5",
  border: "#c0c0c0",
  subtleBorder: "#f5f5f5",
  hoverBg: "#f5f5f5",
  selectedBg: "#c0c0c0",
  accent: "#58f",
  error: "#a44",
  errorText: "#f55",
  success: "#4a4",
  warning: "#da3",
};

function rgb([r, g, b]: [number, number, number]): string {
  return `rgb(${r}, ${g}, ${b})`;
}

/**
 * Chrome drawn out of the palette itself, and nothing else.
 *
 * Every colour here is one of the palette's own entries at full opacity: no
 * tint of the foreground over the background, no translucency, no blend. Those
 * produced neutral greys whatever the palette said, so a carefully chosen
 * scheme still came up with grey panels and grey borders around it.
 *
 * The hierarchy is three of the palette's own tones rather than three degrees
 * of grey: `recede` sits behind the terminal's background (black on a dark
 * palette, bright white on a light one), `raise` sits in front of it, and the
 * background itself is the middle. A panel recedes, an input or a hovered row
 * comes back to the middle, and a selection or a border is raised.
 */
function themeFromPalette(palette: TerminalPalette): Theme {
  const entry = (index: number, fallback: [number, number, number]) =>
    palette.ansi[index] ?? fallback;
  const recede = palette.dark ? entry(0, palette.bg) : entry(15, palette.bg);
  const raise = palette.dark ? entry(8, palette.fg) : entry(7, palette.fg);
  const accent = entry(12, entry(4, palette.fg));

  return {
    bg: rgb(palette.bg),
    fg: rgb(palette.fg),
    // The palette's own dim tone. On both polarities that is the eighth entry:
    // "bright black" is what a scheme picks for text that is present but not
    // being read.
    dimFg: rgb(entry(8, palette.fg)),
    panelBg: rgb(recede),
    solidPanelBg: rgb(recede),
    inputBg: rgb(palette.bg),
    solidInputBg: rgb(palette.bg),
    border: rgb(raise),
    // A hairline of the terminal's background against a receded panel: seen
    // where it separates, unnoticed where it does not.
    subtleBorder: rgb(palette.bg),
    hoverBg: rgb(palette.bg),
    selectedBg: rgb(raise),
    accent: rgb(accent),
    error: rgb(entry(1, palette.fg)),
    errorText: rgb(entry(9, entry(1, palette.fg))),
    success: rgb(entry(2, palette.fg)),
    warning: rgb(entry(3, palette.fg)),
  };
}

export function themeFor(source: boolean | TerminalPalette): Theme {
  if (typeof source === "boolean") {
    return source ? darkTheme : lightTheme;
  }
  return themeFromPalette(source);
}

/**
 * Thin, theme-matched scrollbar for scrollable overlay lists/panels.
 * Spread into the style of any `overflow: auto` container so it renders a
 * subtle scrollbar instead of the chunky native one.
 */
export function scrollbarStyle(theme: Theme): JSX.CSSProperties {
  return {
    "scrollbar-width": "thin",
    "scrollbar-color": `${theme.border} transparent`,
  };
}

export const sidebarWidth = "20em";

/** Centralized z-index scale (increments of 10 for easy insertion). */
export const z = {
  exitedBanner: 10,
  // The status bar's overflow menu: above the workspace it covers, below the
  // overlays it opens.
  statusMenu: 15,
  overlay: 20,
  disconnected: 30,
  debugPanel: 40,
} as const;

// Layout styles that don't depend on the theme.
export const layout: Record<string, JSX.CSSProperties> = {
  overlay: {
    position: "fixed",
    inset: 0,
    display: "flex",
    "align-items": "center",
    "justify-content": "center",
    "background-color": "rgba(0,0,0,0.5)",
    "backdrop-filter": "blur(1px)",
    "-webkit-backdrop-filter": "blur(1px)",
    "z-index": z.overlay,
    width: "100%",
    height: "100%",
    "max-width": "100%",
    "max-height": "100%",
    padding: 0,
    margin: 0,
  },
  workspace: {
    display: "flex",
    "flex-direction": "column",
    height: "100%",
    width: "100%",
  },
  statusBar: {
    display: "flex",
    "align-items": "center",
    "border-top": "1px solid",
    "flex-shrink": 0,
    "user-select": "none",
  },
  termContainer: {
    flex: 1,
    overflow: "hidden",
    position: "relative",
  },
  panel: {
    padding: "16px",
    // % of the backdrop, not vh: OverlayBackdrop sizes itself to the visual
    // viewport, so this cap dodges the software keyboard too. The backdrop
    // raises it to 100% when the band is cramped (keyboard open).
    "max-height": "var(--overlay-panel-cap, 80%)",
    overflow: "auto",
  },
};

// Reusable component styles.
/** Merges style objects in plain JS, before they reach a JSX `style` prop.
 *
 *  Solid's compiler splits a static style object into per-property assignments
 *  and applies a spread in the same object *after* them, so
 *  `style={{ ...ui.btn, padding: 0 }}` silently keeps ui.btn's padding. Only
 *  dynamic values -- a template literal, a ternary -- survive, because those
 *  compile to effects that run later, which makes whether an override lands
 *  depend on whether it happens to be written as a literal. Merging here keeps
 *  ordinary JS semantics: later arguments win, literal or not.
 *
 *  Falsy arguments are skipped, so a conditional base can be passed inline. */
export function mergeStyle(
  ...styles: (JSX.CSSProperties | false | null | undefined)[]
): JSX.CSSProperties {
  return Object.assign({}, ...styles.filter(Boolean));
}

/** `satisfies` rather than an annotation, so the keys stay exact: an index
 *  signature would make a typo like `ui.btnn` resolve to undefined, which
 *  mergeStyle then skips as falsy and drops the base without a word. */
export const ui = {
  btn: {
    background: "none",
    border: "none",
    color: "inherit",
    cursor: "pointer",
    "font-size": "12px",
    "font-family": "inherit",
    opacity: 0.7,
    padding: "2px 6px",
  },
  input: {
    flex: 1,
    padding: "6px 10px",
    "font-size": "14px",
    // The colour of the text it belongs to, which is the palette's. A fixed
    // grey ignored the palette and looked it.
    border: "1px solid currentColor",
    outline: "none",
    "font-family": "inherit",
  },
  badge: {
    "font-size": "10px",
    padding: "1px 6px",
    // Outlined rather than filled: a fixed blue wash is neither the palette's
    // accent nor readable on every background.
    "background-color": "transparent",
    border: "1px solid currentColor",
    color: "inherit",
    "flex-shrink": 0,
    "line-height": 1.5,
  } as JSX.CSSProperties,
  swatch: {
    display: "inline-block",
    width: "14px",
    height: "14px",
  },
  kbd: {
    display: "inline-block",
    padding: "2px 6px",
    "font-size": "12px",
    "font-family": "inherit",
    border: "1px solid currentColor",
    "white-space": "nowrap",
  },
} satisfies Record<string, JSX.CSSProperties>;

export interface OverlayChromeStyles {
  overlay: JSX.CSSProperties;
  panel: JSX.CSSProperties;
  header: JSX.CSSProperties;
  headerCopy: JSX.CSSProperties;
  title: JSX.CSSProperties;
  subtitle: JSX.CSSProperties;
  headerActions: JSX.CSSProperties;
  closeButton: JSX.CSSProperties;
  footer: JSX.CSSProperties;
  actionButton: JSX.CSSProperties;
}

export function overlayChromeStyles(
  theme: Theme,
  dark: boolean,
  scale: UIScale = uiScale(13),
): OverlayChromeStyles {
  return {
    overlay: {
      padding: `${Math.max(12, scale.panelPadding * 2)}px`,
    },
    panel: {
      "background-color": theme.solidPanelBg,
      color: theme.fg,
      border: `1px solid ${theme.border}`,
      "box-shadow": dark
        ? "0 18px 60px rgba(0,0,0,0.45)"
        : "0 18px 60px rgba(0,0,0,0.12)",
      outline: "none",
    },
    header: {
      display: "flex",
      "justify-content": "space-between",
      "align-items": "flex-start",
      gap: `${scale.gap}px`,
      "flex-wrap": "wrap",
      "margin-bottom": `${scale.gap * 2}px`,
    },
    headerCopy: {
      display: "grid",
      gap: `${scale.tightGap}px`,
      "min-width": 0,
    },
    title: {
      margin: 0,
      "font-size": `${scale.xl}px`,
      "line-height": 1.2,
      "font-weight": 600,
    },
    subtitle: {
      margin: 0,
      "font-size": `${scale.sm}px`,
      "line-height": 1.4,
      color: theme.dimFg,
    },
    headerActions: {
      display: "flex",
      "align-items": "center",
      gap: `${scale.tightGap + 2}px`,
      "margin-left": "auto",
    },
    closeButton: {
      ...ui.btn,
      opacity: 0.6,
      padding: `${scale.controlY}px ${scale.controlX}px`,
      border: `1px solid ${theme.subtleBorder}`,
      "background-color": theme.inputBg,
      "font-size": `${scale.sm}px`,
      "white-space": "nowrap",
    },
    footer: {
      display: "flex",
      "justify-content": "space-between",
      "align-items": "center",
      gap: `${scale.gap}px`,
      "flex-wrap": "wrap",
    },
    actionButton: {
      appearance: "none",
      border: `1px solid ${theme.subtleBorder}`,
      "background-color": theme.inputBg,
      color: theme.fg,
      padding: `${scale.controlY + 2}px ${scale.controlX + 2}px`,
      "font-size": `${scale.sm}px`,
      "font-family": "inherit",
      cursor: "pointer",
    },
  };
}

export interface DisconnectedStyles extends OverlayChromeStyles {
  card: JSX.CSSProperties;
  content: JSX.CSSProperties;
  title: JSX.CSSProperties;
  reloadButton: JSX.CSSProperties;
}

export function disconnectedStyles(
  theme: Theme,
  dark: boolean,
  scale: UIScale = uiScale(13),
): DisconnectedStyles {
  const chrome = overlayChromeStyles(theme, dark, scale);

  return {
    ...chrome,
    card: {
      ...chrome.panel,
      width: "min(24em, calc(100vw - 2em))",
      "max-width": "100%",
      background: dark ? theme.solidPanelBg : theme.panelBg,
      padding: 0,
    },
    content: {
      display: "grid",
      gap: "0.75em",
      "justify-items": "center",
      padding: "1.2em 1.4em 1em",
    },
    title: {
      margin: 0,
      "font-size": "1.2em",
      "line-height": 1.2,
      "font-weight": 600,
    },
    reloadButton: {
      ...chrome.actionButton,
      padding: "0.5em 0.75em",
    },
  };
}
