import { PALETTES, type YasWorkspace } from "@yas-run/core";
import { createSignal } from "solid-js";
import { render } from "solid-js/web";
import { afterEach, expect, it, vi } from "vitest";
import { ManageTile } from "../ManageTile";
import { activeEditor } from "../ide/activeEditor";
import { autoFocusPaneTarget } from "../layout/treeContext";
import { themeFor, uiScale } from "../theme";

vi.mock("@yas-run/solid", () => ({
  createYasWorkspaceState: () => () => ({ connections: [], sessions: [] }),
}));

let dispose: (() => void) | undefined;
afterEach(() => {
  dispose?.();
  document.body.replaceChildren();
});

it("takes DOM focus from a terminal when the Manage pane is selected", async () => {
  const terminalPane = document.createElement("div");
  terminalPane.dataset.yasPaneId = "terminal";
  const input = document.createElement("textarea");
  terminalPane.append(input);
  document.body.append(terminalPane);
  input.focus();

  const host = document.createElement("div");
  host.dataset.yasPaneId = "manage";
  document.body.append(host);
  const [focused, setFocused] = createSignal(false);
  const workspace = { getConnection: () => null } as unknown as YasWorkspace;
  dispose = render(
    () => (
      <ManageTile
        workspace={workspace}
        connectionId="dev"
        theme={themeFor(PALETTES[0])}
        palette={PALETTES[0]}
        scale={uiScale(14)}
        fontSize={14}
        focused={focused()}
      />
    ),
    host,
  );
  expect(document.activeElement).toBe(input);

  setFocused(true);
  // LayoutContainer's target selection must find a non-editable target even
  // while the server is disconnected or the active panel has only buttons.
  autoFocusPaneTarget(focused, () =>
    host.querySelector<HTMLElement>("[tabindex], input, textarea"),
  );
  await Promise.resolve();
  expect(document.activeElement).toBe(host.firstElementChild);
  expect(activeEditor()?.kind).toBe("manage");
  expect(document.activeElement?.matches("input, textarea")).toBe(false);
});

it("does not give previews a keyboard focus target", () => {
  dispose = render(
    () => (
      <ManageTile
        workspace={{ getConnection: () => null } as unknown as YasWorkspace}
        connectionId="dev"
        theme={themeFor(PALETTES[0])}
        palette={PALETTES[0]}
        scale={uiScale(14)}
        fontSize={14}
        preview
      />
    ),
    document.body,
  );
  expect(document.querySelector("[tabindex]")).toBeNull();
  expect(activeEditor()).toBeNull();
});
