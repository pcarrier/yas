import { afterEach, describe, expect, it } from "vitest";
import { retainSwitcherFocus, revealSwitcherItem } from "../switcherFocus";

function rect(top: number, bottom: number): DOMRect {
  return {
    top,
    bottom,
    left: 0,
    right: 100,
    width: 100,
    height: bottom - top,
    x: 0,
    y: top,
    toJSON: () => ({}),
  };
}

describe("switcher selection scrolling", () => {
  it("scrolls only its results list to reveal rows above and below", () => {
    const scroller = document.createElement("div");
    const item = document.createElement("button");
    scroller.scrollTop = 50;
    scroller.getBoundingClientRect = () => rect(100, 300);

    item.getBoundingClientRect = () => rect(70, 90);
    revealSwitcherItem(scroller, item);
    expect(scroller.scrollTop).toBe(20);

    item.getBoundingClientRect = () => rect(320, 340);
    revealSwitcherItem(scroller, item);
    expect(scroller.scrollTop).toBe(60);

    item.getBoundingClientRect = () => rect(150, 170);
    revealSwitcherItem(scroller, item);
    expect(scroller.scrollTop).toBe(60);
  });
});

describe("switcher focus ownership", () => {
  afterEach(() => document.body.replaceChildren());

  function fixture() {
    const root = document.createElement("div");
    const search = document.createElement("input");
    root.append(search);
    document.body.append(root);
    const release = retainSwitcherFocus(root, search);
    return { root, search, release };
  }

  it("focuses the search field when opened", () => {
    const { search } = fixture();
    expect(document.activeElement).toBe(search);
  });

  it("focuses after a portal attaches its root", async () => {
    const root = document.createElement("div");
    const search = document.createElement("input");
    root.append(search);
    const release = retainSwitcherFocus(root, search);

    document.body.append(root);
    await Promise.resolve();

    expect(document.activeElement).toBe(search);
    release();
  });

  it("takes focus back from a pane input", () => {
    const { search } = fixture();
    const paneInput = document.createElement("textarea");
    document.body.append(paneInput);

    paneInput.focus();

    expect(document.activeElement).toBe(search);
  });

  it("takes focus back from passive preview inputs", () => {
    const { root, search } = fixture();
    const previewInput = document.createElement("textarea");
    root.append(previewInput);

    previewInput.focus();

    expect(document.activeElement).toBe(search);
  });

  it("allows the switcher's own controls to retain focus", async () => {
    const { root } = fixture();
    const button = document.createElement("button");
    root.append(button);

    button.focus();
    await Promise.resolve();

    expect(document.activeElement).toBe(button);
  });

  it("stops retaining focus after cleanup", () => {
    const { release } = fixture();
    const paneInput = document.createElement("textarea");
    document.body.append(paneInput);

    release();
    paneInput.focus();

    expect(document.activeElement).toBe(paneInput);
  });
});
