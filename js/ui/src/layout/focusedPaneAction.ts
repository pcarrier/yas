/**
 * Resolve the focused pane when a status-bar action is invoked, not when its
 * button was rendered. Removing or floating a pane can rewrite every pane ID;
 * a toolbar instance retained across that rewrite must never act on its stale
 * captured path.
 */
export function focusedPaneAction(
  focusedPaneId: () => string | null,
  action: (paneId: string) => void,
): () => void {
  return () => {
    const paneId = focusedPaneId();
    if (paneId != null) action(paneId);
  };
}
