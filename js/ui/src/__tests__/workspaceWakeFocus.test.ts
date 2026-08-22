import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { disarmPrefix, handlePrefixKey, prefixArmed } from "../keyPrefix";
import { restoreWorkspaceFocusOnWake } from "../workspaceWakeFocus";

describe("workspace focus after returning to the app", () => {
  const cleanup: (() => void)[] = [];

  beforeEach(() => {
    vi.useFakeTimers();
    vi.spyOn(document, "visibilityState", "get").mockReturnValue("visible");
    vi.spyOn(document, "hasFocus").mockReturnValue(true);
    // jsdom has no layout; give attached elements a rendered box.
    vi.spyOn(HTMLElement.prototype, "getClientRects").mockReturnValue([
      {} as DOMRect,
    ] as unknown as DOMRectList);
  });
  afterEach(() => {
    for (const release of cleanup.splice(0)) release();
    document.body.replaceChildren();
    disarmPrefix();
    vi.restoreAllMocks();
    vi.useRealTimers();
  });

  function fixture(canRecoverPane?: () => boolean) {
    const root = document.createElement("main");
    root.tabIndex = -1;
    const input = document.createElement("textarea");
    root.append(input);
    document.body.append(root);
    const findTarget = vi.fn<() => HTMLElement | null>(() => null);
    const release = restoreWorkspaceFocusOnWake(
      root,
      findTarget,
      canRecoverPane,
    );
    cleanup.push(release);
    return { root, input, findTarget, release };
  }

  function visibility(state: DocumentVisibilityState) {
    vi.spyOn(document, "visibilityState", "get").mockReturnValue(state);
    document.dispatchEvent(new Event("visibilitychange"));
  }

  it("restores the previous control when backgrounding dropped DOM focus", () => {
    const { input } = fixture();
    input.focus();
    visibility("hidden");
    input.blur();
    visibility("visible");
    vi.runOnlyPendingTimers();
    expect(document.activeElement).toBe(input);
  });

  it("repairs native focus when the DOM still reports the old input", () => {
    const { root, input } = fixture();
    input.focus();
    visibility("hidden");
    vi.spyOn(document, "hasFocus").mockReturnValue(false);
    const fallbackFocus = vi.spyOn(root, "focus");
    const inputFocus = vi.spyOn(input, "focus");

    visibility("visible");
    vi.runOnlyPendingTimers();
    expect(fallbackFocus).toHaveBeenCalledOnce();
    expect(inputFocus).toHaveBeenCalledWith({ preventScroll: true });
    expect(document.activeElement).toBe(input);
  });

  it("leaves an input with intact browser focus alone", () => {
    const { input } = fixture();
    input.focus();
    const focus = vi.spyOn(input, "focus");
    const blur = vi.spyOn(input, "blur");
    visibility("hidden");
    visibility("visible");
    vi.runOnlyPendingTimers();
    expect(focus).not.toHaveBeenCalled();
    expect(blur).not.toHaveBeenCalled();
  });

  it("preserves a new overlay's focus while foreground events settle", () => {
    const { input } = fixture();
    input.focus();
    visibility("hidden");
    input.blur();
    visibility("visible");
    const search = document.createElement("input");
    document.body.append(search);
    search.focus();
    vi.runOnlyPendingTimers();
    expect(document.activeElement).toBe(search);
  });

  it("uses the current pane when the previous control was removed", () => {
    const { root, input, findTarget } = fixture();
    input.focus();
    visibility("hidden");
    input.remove();
    const next = document.createElement("textarea");
    root.append(next);
    findTarget.mockReturnValue(next);
    visibility("visible");
    vi.runOnlyPendingTimers();
    expect(document.activeElement).toBe(next);
  });

  it.each([false, true])(
    "hands the fallback to the current pane on wake (retained: %s)",
    (retained) => {
      const { root, input, findTarget } = fixture();
      root.focus();
      visibility("hidden");
      if (!retained) root.blur();
      findTarget.mockReturnValue(input);
      visibility("visible");
      vi.runOnlyPendingTimers();
      expect(document.activeElement).toBe(input);
    },
  );

  it("returns to the pane when the focused control is removed without blur", async () => {
    const { input, findTarget } = fixture();
    findTarget.mockReturnValue(input);
    const menu = document.createElement("button");
    document.body.append(menu);
    menu.focus();
    menu.remove();

    await Promise.resolve();
    vi.runOnlyPendingTimers();
    expect(document.activeElement).toBe(input);
  });

  it("hands the fallback to a pane input that mounts later", async () => {
    const { root, input, findTarget } = fixture();
    input.remove();
    root.focus();
    root.append(input);
    findTarget.mockReturnValue(input);

    await Promise.resolve();
    vi.runOnlyPendingTimers();
    expect(document.activeElement).toBe(input);
  });

  it("preserves intervening chrome focus after a control is removed", async () => {
    const { input, findTarget } = fixture();
    findTarget.mockReturnValue(input);
    const menu = document.createElement("button");
    const search = document.createElement("input");
    document.body.append(menu, search);
    menu.focus();
    menu.remove();
    await Promise.resolve();
    search.focus();
    vi.runOnlyPendingTimers();
    expect(document.activeElement).toBe(search);
  });

  it("does not recover pane focus while an overlay owns the keyboard", async () => {
    const { input, findTarget } = fixture(() => false);
    findTarget.mockReturnValue(input);
    const menu = document.createElement("button");
    document.body.append(menu);
    menu.focus();
    menu.remove();

    await Promise.resolve();
    vi.runOnlyPendingTimers();
    expect(document.activeElement).toBe(document.body);
  });

  it("leaves an intentional blur alone while the input remains usable", async () => {
    const { root, input, findTarget } = fixture();
    findTarget.mockReturnValue(input);
    input.focus();
    input.blur();
    root.append(document.createElement("span"));

    await Promise.resolve();
    vi.runOnlyPendingTimers();
    expect(document.activeElement).toBe(document.body);
  });

  it("does not repair a removed control while the window is unfocused", async () => {
    const { root, input, findTarget } = fixture();
    input.focus();
    window.dispatchEvent(new Event("blur"));
    vi.spyOn(document, "hasFocus").mockReturnValue(false);
    input.remove();
    const next = document.createElement("textarea");
    root.append(next);
    findTarget.mockReturnValue(next);

    await Promise.resolve();
    vi.runOnlyPendingTimers();
    expect(document.activeElement).toBe(document.body);
  });

  it("disconnects removal recovery on disposal", async () => {
    const { input, findTarget, release } = fixture();
    findTarget.mockReturnValue(input);
    const menu = document.createElement("button");
    document.body.append(menu);
    menu.focus();
    menu.remove();
    await Promise.resolve();
    release();
    document.body.append(document.createElement("span"));
    await Promise.resolve();
    vi.runOnlyPendingTimers();
    expect(document.activeElement).toBe(document.body);
  });

  it.each(["hidden", "inert"])(
    "uses the non-editable fallback when the old pane is %s",
    (attribute) => {
      const { root, input } = fixture();
      input.focus();
      visibility("hidden");
      input.blur();
      input.setAttribute(attribute, "");
      visibility("visible");
      vi.runOnlyPendingTimers();
      expect(document.activeElement).toBe(root);
    },
  );

  it("restores on window focus even without a visibility transition", () => {
    const { input } = fixture();
    input.focus();
    window.dispatchEvent(new Event("blur"));
    input.blur();
    window.dispatchEvent(new Event("focus"));
    vi.runOnlyPendingTimers();
    expect(document.activeElement).toBe(input);
  });

  it("does not restore while hidden or after disposal", () => {
    const { input, release } = fixture();
    input.focus();
    visibility("hidden");
    input.blur();
    window.dispatchEvent(new Event("focus"));
    vi.runOnlyPendingTimers();
    expect(document.activeElement).toBe(document.body);

    visibility("visible");
    release();
    vi.runOnlyPendingTimers();
    window.dispatchEvent(new Event("focus"));
    vi.runOnlyPendingTimers();
    expect(document.activeElement).toBe(document.body);
  });

  it.each(["blur", "hidden"])("cancels an armed Cmd-B on %s", (event) => {
    fixture();
    vi.spyOn(navigator, "platform", "get").mockReturnValue("MacIntel");
    const commandB = new KeyboardEvent("keydown", {
      key: "b",
      code: "KeyB",
      metaKey: true,
    });
    handlePrefixKey(commandB);
    expect(prefixArmed()).toBe(true);
    if (event === "blur") window.dispatchEvent(new Event("blur"));
    else visibility("hidden");
    expect(prefixArmed()).toBe(false);
    visibility("visible");
    vi.runOnlyPendingTimers();
    handlePrefixKey(commandB);
    expect(prefixArmed()).toBe(true);
  });
});
