import { describe, expect, it } from "vitest";
import { createRoot } from "solid-js";
import { createTabCloseTracker } from "../ide/tabCloseTracker";

describe("createTabCloseTracker", () => {
  it("keeps a closed tab hidden until its registry watch catches up", () => {
    createRoot((dispose) => {
      const tracker = createTabCloseTracker();
      const assignment = "manage:hound:";
      const operation = tracker.begin(assignment);

      expect(tracker.isClosing(assignment)).toBe(true);
      tracker.settle(assignment, operation, true, true);
      tracker.reconcile(new Set([assignment]));
      expect(tracker.isClosing(assignment)).toBe(true);

      tracker.reconcile(new Set());
      expect(tracker.isClosing(assignment)).toBe(false);
      dispose();
    });
  });

  it("restores a tab after a failed deletion", () => {
    createRoot((dispose) => {
      const tracker = createTabCloseTracker();
      const assignment = "manage:hound:";
      const operation = tracker.begin(assignment);

      tracker.settle(assignment, operation, false, true);
      expect(tracker.isClosing(assignment)).toBe(false);
      dispose();
    });
  });

  it("does not let an old close hide a reopened tab", () => {
    createRoot((dispose) => {
      const tracker = createTabCloseTracker();
      const assignment = "manage:hound:";
      const operation = tracker.begin(assignment);

      tracker.reopen(assignment);
      tracker.settle(assignment, operation, true, true);
      expect(tracker.isClosing(assignment)).toBe(false);
      dispose();
    });
  });
});
