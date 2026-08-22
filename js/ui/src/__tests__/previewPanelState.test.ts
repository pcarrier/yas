import { describe, expect, it } from "vitest";
import { previewPanelState } from "../previewPanelState";

describe("previewPanelState", () => {
  it("keeps the status toggle on while hiding an empty shelf", () => {
    expect(previewPanelState(true, false, false)).toEqual({
      enabled: true,
      visible: false,
    });
  });

  it("shows parked items only when enabled", () => {
    expect(previewPanelState(true, true, false).visible).toBe(true);
    expect(previewPanelState(false, true, false).visible).toBe(false);
  });

  it("temporarily reveals the drop target without changing the toggle", () => {
    expect(previewPanelState(false, false, true)).toEqual({
      enabled: false,
      visible: true,
    });
  });
});
