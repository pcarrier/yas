import { describe, expect, it } from "vitest";
import {
  disarmPrefix,
  handlePrefixKey,
  isPrefixChord,
  prefixActionTokens,
  prefixArmed,
  prefixBindings,
  prefixChordLabel,
  prefixToken,
  registerPrefixAction,
} from "../keyPrefix";

function chord(
  key: string,
  options: Partial<
    Pick<KeyboardEvent, "ctrlKey" | "metaKey" | "altKey" | "shiftKey" | "code">
  > = {},
) {
  return {
    key,
    code: options.code ?? "",
    ctrlKey: options.ctrlKey ?? false,
    metaKey: options.metaKey ?? false,
    altKey: options.altKey ?? false,
    shiftKey: options.shiftKey ?? false,
  };
}

const PREFIX = chord("b", { ctrlKey: true });

describe("isPrefixChord", () => {
  it("is Ctrl+B on every platform", () => {
    expect(isPrefixChord(PREFIX, "Linux x86_64")).toBe(true);
    expect(isPrefixChord(chord("B", { ctrlKey: true }), "MacIntel")).toBe(
      true,
    );
    expect(
      isPrefixChord(
        chord("", { ctrlKey: true, code: "KeyB" }),
        "Linux x86_64",
      ),
    ).toBe(true);
  });

  it("also accepts Command+B on Apple platforms", () => {
    expect(
      isPrefixChord(chord("b", { metaKey: true }), "MacIntel"),
    ).toBe(true);
    expect(isPrefixChord(chord("b", { metaKey: true }), "iPad")).toBe(true);
    expect(
      isPrefixChord(chord("b", { metaKey: true }), "Linux x86_64"),
    ).toBe(false);
  });

  it("rejects the prefix chord with any other modifier", () => {
    expect(isPrefixChord(chord("b", { ctrlKey: true, shiftKey: true }))).toBe(
      false,
    );
    expect(isPrefixChord(chord("b", { ctrlKey: true, altKey: true }))).toBe(
      false,
    );
    expect(
      isPrefixChord(
        chord("b", { ctrlKey: true, metaKey: true }),
        "MacIntel",
      ),
    ).toBe(false);
    expect(isPrefixChord(chord("b"))).toBe(false);
  });

  it("uses the compact platform label", () => {
    expect(prefixChordLabel("Linux x86_64")).toBe("ctrl-b");
    expect(prefixChordLabel("MacIntel")).toBe("⌘B");
  });
});

describe("prefixToken", () => {
  it("keeps letter case, so q and Q are different keys", () => {
    expect(prefixToken(chord("q"))).toBe("q");
    expect(prefixToken(chord("Q", { shiftKey: true }))).toBe("Q");
  });

  it("does not read a held modifier as the choice", () => {
    expect(prefixToken(chord("Shift", { shiftKey: true }))).toBeNull();
    expect(prefixToken(chord("Control", { ctrlKey: true }))).toBeNull();
  });

  it("distinguishes Tab from Shift+Tab", () => {
    expect(prefixToken(chord("Tab"))).toBe("Tab");
    expect(prefixToken(chord("Tab", { shiftKey: true }))).toBe("Shift+Tab");
  });

  it("reads brackets off the physical key, for layouts that report otherwise", () => {
    expect(prefixToken(chord("è", { code: "BracketLeft" }))).toBe("[");
    expect(prefixToken(chord("+", { code: "BracketRight" }))).toBe("]");
  });

  it("names the repeated prefix so it can be forwarded", () => {
    expect(prefixToken(PREFIX)).toBe("prefix");
  });
});

describe("handlePrefixKey", () => {
  it("ignores everything until the prefix arrives", () => {
    disarmPrefix();
    expect(handlePrefixKey(chord("q"))).toBe(false);
    expect(prefixArmed()).toBe(false);
  });

  it("arms, runs the bound action, and disarms", () => {
    disarmPrefix();
    let ran = 0;
    const unbind = registerPrefixAction("q", () => ran++);

    expect(handlePrefixKey(PREFIX)).toBe(true);
    expect(prefixArmed()).toBe(true);
    expect(handlePrefixKey(chord("q"))).toBe(true);
    expect(ran).toBe(1);
    expect(prefixArmed()).toBe(false);

    // Disarmed: the same key is the pane's again.
    expect(handlePrefixKey(chord("q"))).toBe(false);
    expect(ran).toBe(1);
    unbind();
  });

  it("swallows an unbound key rather than passing it on", () => {
    disarmPrefix();
    handlePrefixKey(PREFIX);
    expect(handlePrefixKey(chord("ø"))).toBe(true);
    expect(prefixArmed()).toBe(false);
  });

  it("stays armed across a bare modifier press", () => {
    disarmPrefix();
    let ran = 0;
    const unbind = registerPrefixAction("Q", () => ran++);
    handlePrefixKey(PREFIX);
    expect(handlePrefixKey(chord("Shift", { shiftKey: true }))).toBe(true);
    expect(prefixArmed()).toBe(true);
    handlePrefixKey(chord("Q", { shiftKey: true }));
    expect(ran).toBe(1);
    unbind();
  });

  it("cancels on Escape without running anything", () => {
    disarmPrefix();
    let ran = 0;
    const unbind = registerPrefixAction("Escape", () => ran++);
    handlePrefixKey(PREFIX);
    expect(handlePrefixKey(chord("Escape"))).toBe(true);
    expect(prefixArmed()).toBe(false);
    expect(ran).toBe(0);
    unbind();
  });

  it("forwards a repeated prefix through its own binding", () => {
    disarmPrefix();
    let forwarded = 0;
    const unbind = registerPrefixAction("prefix", () => forwarded++);
    handlePrefixKey(PREFIX);
    handlePrefixKey(PREFIX);
    expect(forwarded).toBe(1);
    expect(prefixArmed()).toBe(false);
    unbind();
  });
});

describe("registerPrefixAction", () => {
  it("lets a later binding shadow an earlier one, and restores it", () => {
    disarmPrefix();
    const calls: string[] = [];
    const dropOuter = registerPrefixAction("h", () => calls.push("outer"));
    const dropInner = registerPrefixAction("h", () => calls.push("inner"));

    handlePrefixKey(PREFIX);
    handlePrefixKey(chord("h"));
    expect(calls).toEqual(["inner"]);

    // LayoutContainer unmounts: the workspace's own binding is live again.
    dropInner();
    handlePrefixKey(PREFIX);
    handlePrefixKey(chord("h"));
    expect(calls).toEqual(["inner", "outer"]);

    dropOuter();
    expect(prefixActionTokens()).not.toContain("h");
  });
});

describe("the map behind the prefix", () => {
  it("lists every labelled key, and nothing unlabelled", () => {
    disarmPrefix();
    const drops = [
      registerPrefixAction("k", () => {}, "Menu"),
      registerPrefixAction("#", () => {}, "Symbols"),
      // A binding with no label is machinery, not a key to advertise.
      registerPrefixAction("prefix", () => {}),
    ];
    const map = new Map(
      prefixBindings().map(({ token, label }) => [token, label]),
    );
    expect(map.get("k")).toBe("Menu");
    expect(map.get("#")).toBe("Symbols");
    expect(map.has("prefix")).toBe(false);
    for (const drop of drops) drop();
  });

  it("forgets a key when what registered it goes away", () => {
    disarmPrefix();
    const drop = registerPrefixAction("z", () => {}, "Zoom pane");
    expect(prefixBindings().some((b) => b.token === "z")).toBe(true);
    drop();
    expect(prefixBindings().some((b) => b.token === "z")).toBe(false);
  });

  it("keeps registration order for the visible prefix menu", () => {
    disarmPrefix();
    const drops = [
      registerPrefixAction("prefix", () => {}, "Send prefix"),
      registerPrefixAction("?", () => {}, "Keyboard shortcuts"),
      registerPrefixAction("k", () => {}, "Menu"),
    ];
    expect(prefixBindings().map(({ token }) => token)).toEqual([
      "prefix",
      "?",
      "k",
    ]);
    for (const drop of drops) drop();
  });
});
