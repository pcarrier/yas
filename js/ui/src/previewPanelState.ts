export interface PreviewPanelState {
  /** Persistent status-bar toggle state. */
  enabled: boolean;
  /** Whether the shelf currently occupies screen space. */
  visible: boolean;
}

/**
 * The parked-window shelf has two deliberately different states: enabled is
 * the user's preference, while visible is suppressed when there is nothing to
 * show. A pane drag may reveal it temporarily without changing the preference.
 */
export function previewPanelState(
  enabled: boolean,
  hasItems: boolean,
  paneDragActive: boolean,
): PreviewPanelState {
  return {
    enabled,
    visible: paneDragActive || (enabled && hasItems),
  };
}
