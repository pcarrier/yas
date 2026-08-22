import { FS_ENTRY_DIR, FS_ENTRY_FILE, measureCell } from "@yas-run/core";
import { createSignal } from "solid-js";
import { render } from "solid-js/web";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ExplorerPanel } from "../ide/ExplorerPanel";
import type { IdeSession } from "../ide/session";
import { themeFor, uiScale } from "../theme";

// jsdom has no canvas. Fractional test metrics prove the component delegates
// to the terminal helper without rounding back to a font-size multiplier.
vi.mock("@yas-run/core", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@yas-run/core")>()),
  measureCell: vi.fn(),
}));

const metrics = (height: number) => ({
  w: 8,
  h: height,
  pw: 16,
  ph: height * 2,
});
let dispose: (() => void) | undefined;
const fontsDescriptor = Object.getOwnPropertyDescriptor(document, "fonts");
beforeEach(() => {
  vi.mocked(measureCell).mockImplementation((_family, size) =>
    metrics(size + 3.5),
  );
});
afterEach(() => {
  dispose?.();
  document.body.replaceChildren();
  vi.clearAllMocks();
  if (fontsDescriptor)
    Object.defineProperty(document, "fonts", fontsDescriptor);
  else Reflect.deleteProperty(document, "fonts");
});

function mount(initialSize = 14) {
  const [fontSize, setFontSize] = createSignal(initialSize);
  const [fontFamily, setFontFamily] = createSignal("monospace");
  const filename = "hanging-glyphs.py";
  const directory = "typography";
  const session = {
    connectionId: "fixture",
    root: () => "/fixture",
    ensureTree: () => {},
    fsError: () => null,
    tree: () => [
      {
        name: filename,
        relPath: filename,
        type: FS_ENTRY_FILE,
        flags: 0,
        depth: 0,
        size: 123,
      },
      {
        name: directory,
        relPath: directory,
        type: FS_ENTRY_DIR,
        flags: 0,
        depth: 0,
        expanded: false,
        size: 0,
      },
    ],
    gitHandle: () => ({ workdir: "/fixture" }),
    gitState: () => ({
      head: null,
      op: null,
      status: [
        { path: filename, oldPath: "", staged: 32, unstaged: 77, flags: 0 },
      ],
    }),
  } as unknown as IdeSession;
  dispose = render(
    () => (
      <div style={{ "line-height": 1 }}>
        <ExplorerPanel
          session={session}
          theme={themeFor(true)}
          scale={uiScale(fontSize())}
          fontSize={fontSize()}
          fontFamily={fontFamily()}
          onOpenTile={() => {}}
        />
      </div>
    ),
    document.body,
  );

  const rows = () =>
    Array.from(
      document.querySelectorAll<HTMLElement>(
        `[title="${filename}"], [title="${directory}"]`,
      ),
    );
  return { rows, setFontSize, setFontFamily, filename, directory };
}

describe("Explorer filename typography", () => {
  it.each([12, 13.5, 14, 18, 24])(
    "uses terminal metrics for text and rows at %ipx",
    (fontSize) => {
      const { rows, filename, directory } = mount(fontSize);
      // One changed-file row plus the file and directory entries in the tree.
      expect(rows()).toHaveLength(3);
      expect(measureCell).toHaveBeenCalledWith("monospace", fontSize);
      expect(measureCell).toHaveBeenCalledWith(
        "monospace",
        uiScale(fontSize).sm,
      );
      expect(measureCell).toHaveBeenCalledTimes(2);
      rows().forEach((row, index) => {
        const renderedSize = index === 0 ? uiScale(fontSize).sm : fontSize;
        expect(row.style.lineHeight).toBe(`${renderedSize + 3.5}px`);
        expect(row.style.fontSize).toBe(`${renderedSize}px`);
        expect(row.style.height).toBe(`${fontSize + 3.5}px`);
        expect(row.style.whiteSpace).toBe("nowrap");
        const label = Array.from(row.children).find(
          (el) => el.textContent === filename || el.textContent === directory,
        ) as HTMLElement;
        // Keep horizontal truncation: removing overflow would only mask the
        // vertical clipping at the cost of long names escaping the sidebar.
        expect(label.style.overflow).toBe("hidden");
        expect(label.style.textOverflow).toBe("ellipsis");
      });
    },
  );

  it("remeasures when the font family or size changes", () => {
    const { rows, setFontSize, setFontFamily } = mount();
    setFontSize(18);
    expect(rows()[1].style.height).toBe("21.5px");
    expect(rows()[0].style.lineHeight).toBe(`${uiScale(18).sm + 3.5}px`);
    setFontFamily("New Font");
    expect(measureCell).toHaveBeenCalledWith("New Font", 18);
    expect(measureCell).toHaveBeenCalledWith("New Font", uiScale(18).sm);
  });

  it("refreshes loaded-font and display metrics, and removes its listeners on unmount", () => {
    const fonts = new EventTarget();
    Object.defineProperty(document, "fonts", {
      configurable: true,
      value: fonts,
    });
    const { rows } = mount();
    vi.mocked(measureCell).mockImplementation((_family, size) =>
      metrics(size + 5),
    );
    fonts.dispatchEvent(new Event("loadingdone"));
    expect(rows()[1].style.height).toBe("19px");
    expect(rows()[0].style.lineHeight).toBe("17px");
    vi.mocked(measureCell).mockImplementation((_family, size) =>
      metrics(size + 5.5),
    );
    window.dispatchEvent(new Event("resize"));
    expect(rows()[1].style.height).toBe("19.5px");
    dispose?.();
    dispose = undefined;
    vi.mocked(measureCell).mockClear();
    fonts.dispatchEvent(new Event("loadingdone"));
    window.dispatchEvent(new Event("resize"));
    expect(measureCell).not.toHaveBeenCalled();
  });
});
