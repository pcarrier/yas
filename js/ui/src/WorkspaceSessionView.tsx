import { Show, type Accessor, type JSX } from "solid-js";
import type {
  WorkspaceSessionBinding,
  WorkspaceSessionController,
} from "./workspaceSession";
import { workspaceSessionBoundary } from "./workspaceSessionBoundary";
import { WorkspaceSessionTabs } from "./WorkspaceSessionTabs";
import { WorkspaceSessionOverlay } from "./WorkspaceSessionOverlay";
import { YasMark } from "./Logo";
import { preferredFont, preferredFontSize, preferredPalette } from "./storage";
import { layout, themeFor } from "./theme";
import { t } from "./i18n";

/** Select the screen without replacing the protocol workspace that owns it. */
export function WorkspaceSessionView(props: {
  session:
    | WorkspaceSessionBinding
    | Accessor<WorkspaceSessionBinding | null>
    | undefined;
  controller?: WorkspaceSessionController;
  children: (session?: WorkspaceSessionBinding) => JSX.Element;
}) {
  const boundary = workspaceSessionBoundary(props.session);
  return boundary.managed ? (
    <Show
      when={boundary.current()}
      keyed
      fallback={<WorkspaceSessionPlaceholder controller={props.controller} />}
    >
      {(session) => props.children(session)}
    </Show>
  ) : (
    props.children(undefined)
  );
}

/**
 * An unbound screen would read the device's legacy layout, place live windows,
 * and resize them before the authoritative workspace arrives. Keep only
 * session controls mounted while loading or without a selected workspace.
 */
function WorkspaceSessionPlaceholder(props: {
  controller?: WorkspaceSessionController;
}) {
  const palette = preferredPalette();
  const theme = themeFor(palette);
  const fontFamily = preferredFont();
  const fontSize = preferredFontSize();
  const isMobileTouch =
    "ontouchstart" in window ||
    navigator.maxTouchPoints > 0 ||
    (window.matchMedia?.("(pointer: coarse)").matches ?? false);

  return (
    <main
      style={{
        ...layout.workspace,
        "background-color": theme.bg,
        color: theme.fg,
        "font-family": fontFamily,
      }}
    >
      <Show when={props.controller}>
        {(controller) => (
          <>
            <WorkspaceSessionTabs
              controller={controller()}
              palette={palette}
              fontFamily={fontFamily}
              fontSize={fontSize}
              isMobileTouch={isMobileTouch}
            />
            <WorkspaceSessionOverlay
              controller={controller()}
              palette={palette}
              fontFamily={fontFamily}
              fontSize={fontSize}
            />
          </>
        )}
      </Show>
      <Show when={props.controller?.loading() ?? true}>
        <div
          role="status"
          aria-label={t("app.loading")}
          aria-busy="true"
          style={{
            flex: 1,
            display: "grid",
            "place-items": "center",
            color: theme.dimFg,
          }}
        >
          <YasMark size={72} />
        </div>
      </Show>
    </main>
  );
}
