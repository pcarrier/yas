import { PALETTES } from "@yas-run/core";
import type { LayoutNode, LayoutSplit } from "@yas-run/core/layout";
import { createSignal } from "solid-js";
import { render } from "solid-js/web";
import { afterEach, describe, expect, it, vi } from "vitest";
import { LayoutContainer } from "../layout/LayoutContainer";
import type { WorkspaceLayout } from "../layout/store";

vi.mock("@yas-run/solid", () => {
  const snapshot = { sessions: [], connections: [], focusedSessionId: null };
  const workspace = { getConnection: () => null, setVisibleSessions: () => {} };
  return {
    createYasWorkspace: () => workspace,
    createYasWorkspaceState: () => () => snapshot,
    createYasSessions: () => () => snapshot.sessions,
    YasTerminal: () => null,
    YasSurfaceView: () => null,
  };
});

let dispose: (() => void) | undefined;
afterEach(() => {
  dispose?.();
  document.body.replaceChildren();
});

describe.each(["horizontal", "vertical"] as const)(
  "%s divider dragging",
  (direction) => {
    it.each([
      { weights: [1, 1], nested: false },
      { weights: [1, 1, 1], nested: false },
      { weights: [1, 2, 3, 4], nested: false },
      { weights: [1, 2, 3], nested: true },
    ])(
      "tracks the pointer with $weights (nested: $nested)",
      ({ weights, nested }) => {
        const split: LayoutSplit = {
          type: "split",
          direction,
          children: weights.map((weight) => ({
            node: { type: "leaf" },
            weight,
          })),
        };
        const root: LayoutNode = nested
          ? {
              type: "split",
              direction: direction === "horizontal" ? "vertical" : "horizontal",
              children: [
                { node: { type: "leaf" }, weight: 1 },
                { node: split, weight: 1 },
              ],
            }
          : split;
        const [layout, setLayout] = createSignal<WorkspaceLayout>({
          name: "Resize regression",
          root,
        });
        dispose = render(
          () => (
            <LayoutContainer
              layout={layout()}
              onLayoutChange={(next) => next && setLayout(next)}
              connectionId="dev"
              palette={PALETTES[0]}
              fontFamily="monospace"
              fontSize={14}
              focusedSessionId={null}
              lruSessionIds={[]}
              storedAssignments={{}}
              onFocusSession={() => {}}
            />
          ),
          document.body,
        );

        const horizontal = direction === "horizontal";
        const handle = [...document.querySelectorAll<HTMLElement>("div")].find(
          (element) =>
            element.style.cursor === (horizontal ? "col-resize" : "row-resize"),
        )!;
        const container = handle.parentElement!.parentElement!;
        const extent = 900;
        Object.defineProperty(
          container,
          horizontal ? "clientWidth" : "clientHeight",
          {
            value: extent,
          },
        );
        handle.setPointerCapture = vi.fn();
        const axis = horizontal ? "clientX" : "clientY";
        const totalWeight = weights.reduce((sum, weight) => sum + weight, 0);
        const initialBoundary = (extent * weights[0]) / totalWeight;
        const origin = 200 + initialBoundary;
        const pointer = (type: string, position: number, pointerId = 7) => {
          const event = new MouseEvent(type, {
            bubbles: true,
            [axis]: position,
          });
          Object.defineProperties(event, {
            pointerId: { value: pointerId },
            pointerType: { value: pointerId === 7 ? "touch" : "mouse" },
          });
          return event;
        };
        handle.dispatchEvent(pointer("pointerdown", origin));

        for (const movement of [90, 45, -30, 0]) {
          const beforeMouse = layout();
          handle.dispatchEvent(pointer("pointerdown", 100, 1));
          document.dispatchEvent(pointer("pointermove", 300, 1));
          document.dispatchEvent(pointer("pointerup", 300, 1));
          expect(layout()).toBe(beforeMouse);

          document.dispatchEvent(pointer("pointermove", origin + movement));
          const currentRoot = layout().root as LayoutSplit;
          const current = nested
            ? (currentRoot.children[1].node as LayoutSplit)
            : currentRoot;
          const currentWeights = current.children.map((child) => child.weight);
          const boundary =
            (extent * currentWeights[0]) /
            currentWeights.reduce((sum, weight) => sum + weight, 0);
          expect(boundary - initialBoundary).toBeCloseTo(movement);
          expect(currentWeights.slice(2)).toEqual(weights.slice(2));
        }
      },
    );
  },
);
