import { describe, expect, it, vi } from "vitest";
import { focusedPaneAction } from "../layout/focusedPaneAction";

describe("focusedPaneAction", () => {
  it("resolves the current pane after each structural rekey", () => {
    let focused: string | null = "0.1";
    const action = vi.fn();
    const invoke = focusedPaneAction(() => focused, action);

    invoke();
    focused = "0";
    invoke();
    focused = "1.2";
    invoke();
    focused = null;
    invoke();

    expect(action.mock.calls).toEqual([["0.1"], ["0"], ["1.2"]]);
  });
});
