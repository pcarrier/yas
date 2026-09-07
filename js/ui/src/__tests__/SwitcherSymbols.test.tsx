import { PALETTES } from "@yas-run/core";
import { createSignal } from "solid-js";
import { render } from "solid-js/web";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { SwitcherOverlay } from "../SwitcherOverlay";

const workspace = vi.hoisted(() => ({
  search: vi.fn().mockResolvedValue([]),
}));

vi.mock("@yas-run/solid", () => ({
  createYasWorkspace: () => workspace,
  YasTerminal: () => null,
  YasSurfaceView: () => null,
}));

vi.mock("../xdgDesktopCatalogs", () => ({
  xdgDesktopCatalogs: () => [],
  applicationIcon: () => undefined,
  requestApplicationIcons: () => {},
  startApplication: vi.fn(),
}));

let dispose: (() => void) | undefined;
const scrollDescriptor = Object.getOwnPropertyDescriptor(
  HTMLElement.prototype,
  "scrollIntoView",
);

beforeEach(() => {
  vi.useFakeTimers();
  vi.stubGlobal("matchMedia", () => ({
    matches: false,
    addEventListener: () => {},
    removeEventListener: () => {},
  }));
  Object.defineProperty(HTMLElement.prototype, "scrollIntoView", {
    configurable: true,
    value: vi.fn(),
  });
});

afterEach(() => {
  dispose?.();
  dispose = undefined;
  document.body.replaceChildren();
  vi.useRealTimers();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
  if (scrollDescriptor) {
    Object.defineProperty(
      HTMLElement.prototype,
      "scrollIntoView",
      scrollDescriptor,
    );
  } else {
    Reflect.deleteProperty(HTMLElement.prototype, "scrollIntoView");
  }
});

describe("switcher symbol search", () => {
  it("retries an unchanged query when the language server becomes ready", async () => {
    const [generation, setGeneration] = createSignal(0);
    const release = vi.fn();
    const search = vi.fn(async () =>
      generation() === 0
        ? []
        : [
            {
              name: "needleSymbol",
              symKind: 12,
              path: "src/needle.ts",
              line: 4,
              col: 2,
            },
          ],
    );

    dispose = render(
      () => (
        <SwitcherOverlay
          sessions={[]}
          focusedSessionId={null}
          lru={[]}
          palette={PALETTES[0]}
          initialQuery="#needle"
          onSelect={() => {}}
          onCreate={() => {}}
          onClose={() => {}}
          symbolSearchWarm={() => release}
          symbolSearchGeneration={generation}
          symbolSearch={search}
        />
      ),
      document.body,
    );

    await vi.advanceTimersByTimeAsync(120);
    expect(search).toHaveBeenCalledTimes(1);
    expect(document.body.textContent).not.toContain("needleSymbol");

    // No query input changes: only the attachment/index generation advances.
    setGeneration(1);
    await vi.advanceTimersByTimeAsync(120);
    expect(search).toHaveBeenCalledTimes(2);
    expect(document.body.textContent).toContain("needleSymbol");

    dispose();
    dispose = undefined;
    expect(release).toHaveBeenCalledOnce();
  });
});
