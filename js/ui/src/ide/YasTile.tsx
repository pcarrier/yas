/**
 * YasTile — renders an IDE tile assignment string as the appropriate view.
 *
 * A tile assignment is one of `editor:<conn>:<path>`, `diff:<conn>:<path>`
 * (optionally `diff:<conn>:staged:<path>`), `commit:<conn>:<oid>:<repoPath>`, or
 * `manage:<conn>:` (see js/core/src/layout/tree.ts). This component parses the
 * assignment and renders YasEditor / YasDiff / YasCommit / ManageTile
 * accordingly.
 *
 * It is the single render path shared by layout leaf panes (LayoutContainer) and the
 * single-view "focused tile" view (Workspace), so the two never drift.
 *
 * Keyed on the assignment string: replacing an editor tile with a *different*
 * file (or an editor with a diff) must rebuild the view, not reuse the old one.
 * YasEditor/YasDiff capture their path at construction, so without this a
 * pane swapped to another file would keep showing the old one. Theme/size props
 * stay reactive (SolidJS tracks them in the JSX), so re-theming never rebuilds.
 *
 * Also the app's error boundary. A throw anywhere inside a tile — a bad
 * assignment, a codec surprise, a grammar that dislikes a file — would
 * otherwise unwind past every pane and blank the whole window, because a
 * Solid error propagates to the nearest boundary and there is none above
 * this one. Containing it here costs one pane and keeps the rest of the
 * layout, and every terminal in it, alive.
 */

import { ErrorBoundary, Show } from "solid-js";
import type {
  YasWorkspace,
  ConnectionId,
  TerminalPalette,
} from "@yas-run/core";
import { parseTileAssignment, parseDiffArg } from "@yas-run/core/layout";
import type { Theme, UIScale } from "../theme";
import { ui } from "../theme";
import { YasDiff } from "./YasDiff";
import { YasEditor } from "./YasEditor";
import { YasCommit } from "./YasCommit";
import { YasPreview } from "./YasPreview";
import { ManageTile } from "../ManageTile";

export function YasTile(props: {
  workspace: YasWorkspace;
  /** The tile assignment string (editor:/diff:/commit:). */
  assignment: string;
  theme: Theme;
  palette: TerminalPalette;
  scale: UIScale;
  fontFamily: string;
  fontSize: number;
  /** Open a further assignment (e.g. a commit file or Muster terminal). */
  onOpenTile: (assignment: string) => void;
  /** Whether a connection is an `.ro` share. Only a manage tile asks: its
   *  clients panel talks a family the share forwarder drops, so offering it
   *  there would sit on "Loading clients…" forever. */
  isConnectionReadOnly?: (connectionId: string) => boolean;
  /** Read-only preview (the background dock): no editing, no LSP, no
   *  buffer parking — a zoomed-out always-on view, like a terminal
   *  thumbnail. */
  preview?: boolean;
  /** Whether this tile owns workspace focus. panes stay mounted in the
   *  background, so status-bar chrome must follow this separately. */
  focused?: boolean;
}) {
  const view = (assignment: string) => {
    const t = parseTileAssignment(assignment);
    if (!t) return null;
    if (t.kind === "diff") {
      const { path, side } = parseDiffArg(t.arg);
      return (
        <YasDiff
          workspace={props.workspace}
          connectionId={t.connectionId}
          path={path}
          side={side}
          theme={props.theme}
          palette={props.palette}
          scale={props.scale}
          fontFamily={props.fontFamily}
          fontSize={props.fontSize}
          onOpenTile={props.onOpenTile}
          preview={props.preview}
          focused={props.focused}
        />
      );
    }
    if (t.kind === "preview") {
      return (
        <YasPreview
          workspace={props.workspace}
          connectionId={t.connectionId}
          path={t.arg}
          theme={props.theme}
          scale={props.scale}
          fontFamily={props.fontFamily}
          fontSize={props.fontSize}
          onOpenTile={props.onOpenTile}
          preview={props.preview}
          focused={props.focused}
        />
      );
    }
    if (t.kind === "manage") {
      return (
        <ManageTile
          workspace={props.workspace}
          connectionId={t.connectionId as ConnectionId}
          theme={props.theme}
          palette={props.palette}
          scale={props.scale}
          fontSize={props.fontSize}
          onOpenAssignment={props.onOpenTile}
          readOnly={props.isConnectionReadOnly?.(t.connectionId)}
          preview={props.preview}
          focused={props.focused}
        />
      );
    }
    if (t.kind === "commit") {
      const colon = t.arg.indexOf(":");
      return (
        <YasCommit
          workspace={props.workspace}
          connectionId={t.connectionId}
          oid={t.arg.slice(0, colon)}
          repoPath={t.arg.slice(colon + 1)}
          theme={props.theme}
          palette={props.palette}
          scale={props.scale}
          fontFamily={props.fontFamily}
          fontSize={props.fontSize}
          onOpenTile={props.onOpenTile}
          preview={props.preview}
          focused={props.focused}
        />
      );
    }
    return (
      <YasEditor
        workspace={props.workspace}
        connectionId={t.connectionId}
        path={t.arg}
        theme={props.theme}
        palette={props.palette}
        fontFamily={props.fontFamily}
        fontSize={props.fontSize}
        onOpenTile={props.onOpenTile}
        preview={props.preview}
        focused={props.focused}
      />
    );
  };

  return (
    <Show when={props.assignment} keyed>
      {(assignment) => (
        <ErrorBoundary
          fallback={(err: unknown, reset: () => void) => (
            <TileError
              assignment={assignment}
              err={err}
              reset={reset}
              theme={props.theme}
              scale={props.scale}
              fontFamily={props.fontFamily}
              preview={props.preview}
            />
          )}
        >
          {view(assignment)}
        </ErrorBoundary>
      )}
    </Show>
  );
}

/** What a pane shows when its tile threw: what broke, where, and a way back. */
function TileError(props: {
  assignment: string;
  err: unknown;
  reset: () => void;
  theme: Theme;
  scale: UIScale;
  fontFamily: string;
  preview?: boolean;
}) {
  const message = () =>
    props.err instanceof Error
      ? props.err.message || props.err.name
      : String(props.err);
  return (
    <div
      style={{
        width: "100%",
        height: "100%",
        display: "flex",
        "flex-direction": "column",
        gap: `${props.scale.tightGap}px`,
        padding: `${props.scale.panelPadding}px`,
        overflow: "auto",
        background: props.theme.bg,
        color: props.theme.fg,
        "font-family": props.fontFamily,
        "font-size": `${props.scale.md}px`,
      }}
    >
      <b style={{ color: props.theme.errorText }}>This pane failed to render</b>
      <div style={{ color: props.theme.dimFg, "word-break": "break-all" }}>
        {props.assignment}
      </div>
      <div style={{ "white-space": "pre-wrap", "word-break": "break-word" }}>
        {message()}
      </div>
      {/* A preview thumbnail has no keyboard path to act on this, so the
          button would be decoration. */}
      <Show when={!props.preview}>
        <div>
          <button style={ui.btn} onClick={() => props.reset()}>
            Retry
          </button>
        </div>
      </Show>
    </div>
  );
}
