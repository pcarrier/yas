import { afterEach, describe, expect, it } from "vitest";
import { retainSwitcherFocus } from "../switcherFocus";

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
