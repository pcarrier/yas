import { afterEach, describe, expect, it } from "vitest";
import {
  dropConnectionTabState,
  pickTab,
  pickedTab,
  setShownTab,
  shownTab,
} from "../connectionTab";
import {
  dropEditorPositions,
  editorPositionCacheStats,
  recallEditorPosition,
  rememberEditorPosition,
} from "../ide/editorPositions";

afterEach(() => {
  dropConnectionTabState("peer-a");
  dropConnectionTabState("peer-b");
  dropEditorPositions("peer-a");
  dropEditorPositions("peer-b");
});

describe("connection-scoped UI cache teardown", () => {
  it("drops panel signals for a route while preserving another route", () => {
    pickTab("peer-a", "extensions");
    setShownTab("peer-a", "session");
    pickTab("peer-b", "clients");

    dropConnectionTabState("peer-a");

    expect(pickedTab("peer-a")).toBeNull();
    expect(shownTab("peer-a")).toBeNull();
    expect(pickedTab("peer-b")).toBe("clients");
  });

  it("drops cursor history for a removed route and bounds oversized keys", () => {
    rememberEditorPosition("peer-a", "/a.ts", {
      anchor: 1,
      head: 2,
      top: 3,
    });
    rememberEditorPosition("peer-b", "/b.ts", {
      anchor: 4,
      head: 5,
      top: 6,
    });
    rememberEditorPosition("peer-a", `/${"x".repeat(9_000)}`, {
      anchor: 7,
      head: 8,
      top: 9,
    });

    dropEditorPositions("peer-a");

    expect(recallEditorPosition("peer-a", "/a.ts")).toBeNull();
    expect(recallEditorPosition("peer-b", "/b.ts")).toEqual({
      anchor: 4,
      head: 5,
      top: 6,
    });
    expect(editorPositionCacheStats().items).toBe(1);
  });
});
