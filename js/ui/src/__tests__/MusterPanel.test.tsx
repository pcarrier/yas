import { PALETTES, type YasWorkspace } from "@yas-run/core";
import { render } from "solid-js/web";
import { afterEach, describe, expect, it, vi } from "vitest";
import { MusterPanel } from "../MusterPanel";
import { followMuster, type MusterHandle, type MusterUnit } from "../muster";
import { pillColor } from "../panelTone";
import { themeFor } from "../theme";

vi.mock("../muster", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../muster")>()),
  followMuster: vi.fn(),
}));

let dispose: (() => void) | undefined;
afterEach(() => {
  dispose?.();
  dispose = undefined;
  document.body.replaceChildren();
  vi.clearAllMocks();
});

function mountUnit(type: MusterUnit["type"], phase: MusterUnit["phase"]) {
  const unit: MusterUnit = {
    name: "test-unit",
    instance: null,
    description: "",
    type,
    phase,
    pty: null,
    restarts: 0,
    lastExit: null,
    requires: [],
    autostart: true,
    stale: false,
    surfaces: [],
    runs: [],
  };
  const handle: MusterHandle = {
    units: new Map([[unit.name, unit]]),
    instances: new Map(),
    dir: "/test/muster",
    ready: true,
    revision: 0,
    events: [],
    subscribe: () => () => {},
    start: vi.fn(),
    stop: vi.fn(),
    restart: vi.fn(),
    rewatch: vi.fn(),
    resync: vi.fn(),
    close: vi.fn(),
  };
  vi.mocked(followMuster).mockImplementation((_connection, options) => {
    options.onHandle(handle);
    return () => {};
  });
  dispose = render(
    () => (
      <MusterPanel
        workspace={{} as YasWorkspace}
        connectionId="test"
        palette={PALETTES[0]}
        fontSize={13}
        sessions={[]}
      />
    ),
    document.body,
  );
  const row = document.querySelector('[data-muster-unit="test-unit"]')!;
  return row.nextElementSibling!.firstElementChild as HTMLElement;
}

describe("MusterPanel phase labels", () => {
  it.each(["oneshot", "simple"] as const)(
    "shows an activating %s as activating with a warning tone",
    (type) => {
      const pill = mountUnit(type, "activating");
      expect(pill.textContent).toBe("activating");
      const dot = pill.querySelector<HTMLElement>('[aria-hidden="true"]')!;
      const expected = document.createElement("span");
      expected.style.backgroundColor = pillColor(themeFor(PALETTES[0]), "warn");
      expect(dot.style.backgroundColor).toBe(expected.style.backgroundColor);
      expect(document.body.textContent).not.toContain("running");
    },
  );

  it("keeps a ready service labeled running", () => {
    expect(mountUnit("simple", "running").textContent).toBe("running");
    expect(document.body.textContent).toContain("1 running");
  });

  it("keeps a successful oneshot labeled done", () => {
    expect(mountUnit("oneshot", "exited").textContent).toBe("done");
    expect(document.body.textContent).not.toContain("running");
  });
});
