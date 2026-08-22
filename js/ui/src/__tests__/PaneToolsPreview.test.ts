import { createMemo, createRoot, type Accessor } from "solid-js";
import { describe, expect, it } from "vitest";
import {
  paneToolDropPreviewController,
  type PaneToolDragSource,
} from "../ide/tileDrag";

function paneFixture(source: PaneToolDragSource): {
  dispose: () => void;
} {
  const pane = document.createElement("div");
  const toolbar = document.createElement("div");
  toolbar.dataset.yasPaneToolsAssignment = source.assignment;
  toolbar.dataset.yasPaneToolsPaneId = source.paneId;
  pane.appendChild(toolbar);
  document.body.appendChild(pane);
  Object.defineProperty(toolbar, "offsetParent", {
    configurable: true,
    get: () => pane,
  });
  pane.getBoundingClientRect = () => ({
    x: 0,
    y: 0,
    top: 0,
    right: 400,
    bottom: 400,
    left: 0,
    width: 400,
    height: 400,
    toJSON: () => ({}),
  });
  return { dispose: () => pane.remove() };
}

function cornerOwner(source: PaneToolDragSource): {
  corner: Accessor<string>;
  dispose: () => void;
} {
  return createRoot((dispose) => ({
    corner: createMemo(() =>
      paneToolDropPreviewController.displayedCorner(source),
    ),
    dispose,
  }));
}

describe("PaneTools shared drag preview", () => {
  it("survives owner replacement and commits through the replacement", () => {
    const source = {
      assignment: "terminal:test-preview-remount",
      paneId: "main-view",
    };
    const fixture = paneFixture(source);
    const first = cornerOwner(source);

    try {
      paneToolDropPreviewController.start(source, {
        clientX: 350,
        clientY: 50,
      });
      expect(first.corner()).toBe("top-right");

      paneToolDropPreviewController.update({ clientX: 50, clientY: 350 });
      expect(first.corner()).toBe("bottom-left");

      // The drag target's structural update disposes the source PaneTools.
      // A replacement owner must immediately consume the in-flight signal.
      first.dispose();
      const replacement = cornerOwner(source);
      try {
        expect(replacement.corner()).toBe("bottom-left");

        paneToolDropPreviewController.finish(source, {
          clientX: 50,
          clientY: 350,
        });
        expect(replacement.corner()).toBe("bottom-left");

        // Cancellation previews another corner but restores the committed
        // one; it must never turn a preview into a saved toolbar location.
        paneToolDropPreviewController.start(source, {
          clientX: 350,
          clientY: 50,
        });
        expect(replacement.corner()).toBe("top-right");
        paneToolDropPreviewController.cancel(source);
        expect(replacement.corner()).toBe("bottom-left");
      } finally {
        replacement.dispose();
      }
    } finally {
      paneToolDropPreviewController.cancel(source);
      fixture.dispose();
    }
  });
});
