import { afterEach, describe, expect, it } from "vitest";
import {
  autoFocusPaneTarget,
  canAutoFocusPane,
  canRestorePaneKeyboardFocus,
} from "../layout/treeContext";

describe("layout focus ownership", () => {
  afterEach(() => document.body.replaceChildren());

  it("does not steal focus from a control outside every pane", () => {
    const chromeButton = document.createElement("button");
    document.body.append(chromeButton);
    chromeButton.focus();

    expect(document.activeElement).toBe(chromeButton);
    expect(canAutoFocusPane(document.activeElement, document.body)).toBe(false);

    chromeButton.remove();
  });

  it("allows focus to hand off from the previously focused pane", () => {
    const previousPane = document.createElement("div");
    previousPane.dataset.yasPaneId = "0";
    const previousInput = document.createElement("textarea");
    previousPane.append(previousInput);
    document.body.append(previousPane);
    previousInput.focus();

    expect(document.activeElement).toBe(previousInput);
    expect(canAutoFocusPane(document.activeElement, document.body)).toBe(true);

    previousPane.remove();
  });

  it("allows focus to hand off from a body-portaled web pane", () => {
    const portalRoot = document.createElement("div");
    portalRoot.dataset.yasWorkspaceFocusOwner = "web-pane";
    const frame = document.createElement("iframe");
    portalRoot.append(frame);
    document.body.append(portalRoot);
    frame.focus();

    expect(document.activeElement).toBe(frame);
    expect(canAutoFocusPane(document.activeElement, document.body)).toBe(true);

    const nextInput = document.createElement("textarea");
    document.body.append(nextInput);
    autoFocusPaneTarget(
      () => true,
      () => nextInput,
      document,
    );
    expect(document.activeElement).toBe(nextInput);
  });

  it("recognizes a focused target through its shadow host", () => {
    const pane = document.createElement("div");
    pane.dataset.yasPaneId = "0";
    const host = document.createElement("div");
    const shadow = host.attachShadow({ mode: "open" });
    const input = document.createElement("input");
    shadow.append(input);
    pane.append(host);
    document.body.append(pane);
    input.focus();

    expect(document.activeElement).toBe(host);
    expect(canAutoFocusPane(document.activeElement, document.body)).toBe(true);
  });

  it("allows initial unowned body focus", () => {
    expect(canAutoFocusPane(document.body, document.body)).toBe(true);
    expect(canAutoFocusPane(null, document.body)).toBe(true);
  });

  it("allows a pane to take over from the workspace keyboard fallback", () => {
    const root = document.createElement("main");
    root.tabIndex = -1;
    root.dataset.yasKeyboardFallback = "";
    const input = document.createElement("input");
    root.append(input);
    document.body.append(root);
    root.focus();
    expect(canAutoFocusPane(document.activeElement, document.body)).toBe(true);

    // The root's marker must not make every control inside it unowned.
    input.focus();
    expect(canAutoFocusPane(document.activeElement, document.body)).toBe(false);
  });

  it("focuses a target attached after the pane effect", async () => {
    let input: HTMLInputElement | null = null;
    autoFocusPaneTarget(
      () => true,
      () => input,
      document,
    );
    input = document.createElement("input");
    document.body.append(input);

    await Promise.resolve();
    expect(document.activeElement).toBe(input);
  });

  it("does not let an empty pane retry steal intervening chrome focus", async () => {
    let input: HTMLInputElement | null = null;
    autoFocusPaneTarget(
      () => true,
      () => input,
      document,
    );

    const chromeButton = document.createElement("button");
    input = document.createElement("input");
    document.body.append(chromeButton, input);
    chromeButton.focus();

    await Promise.resolve();
    expect(document.activeElement).toBe(chromeButton);
  });

  it("does not let a legacy main-view update steal chrome focus", () => {
    const main = document.createElement("div");
    main.dataset.yasWorkspaceFocusOwner = "main";
    const input = document.createElement("textarea");
    const chromeButton = document.createElement("button");
    main.append(input);
    document.body.append(main, chromeButton);
    chromeButton.focus();

    autoFocusPaneTarget(
      () => true,
      () => input,
      document,
    );

    expect(document.activeElement).toBe(chromeButton);
  });

  it("does not let a retry from a formerly focused pane take focus", async () => {
    let focused = true;
    let input: HTMLInputElement | null = null;
    autoFocusPaneTarget(
      () => focused,
      () => input,
      document,
    );
    focused = false;
    input = document.createElement("input");
    document.body.append(input);

    await Promise.resolve();
    expect(document.activeElement).toBe(document.body);
  });

  it("does not restore the keyboard after the user selects another pane", () => {
    const pane = document.createElement("div");
    pane.dataset.yasPaneId = "0";
    pane.dataset.yasPaneFocused = "true";
    const input = document.createElement("textarea");
    pane.append(input);
    document.body.append(pane);
    expect(canRestorePaneKeyboardFocus(input)).toBe(true);

    // The blur schedules a keep-alive before the tab's click selects Manage.
    // At timer execution the old terminal must no longer be a fallback.
    delete pane.dataset.yasPaneFocused;
    expect(canRestorePaneKeyboardFocus(input)).toBe(false);
  });

  it("does not restore a keyboard input removed during pane replacement", () => {
    const input = document.createElement("textarea");
    document.body.append(input);
    expect(canRestorePaneKeyboardFocus(input)).toBe(true);
    input.remove();
    expect(canRestorePaneKeyboardFocus(input)).toBe(false);
  });
});
