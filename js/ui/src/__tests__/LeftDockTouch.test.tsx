import { createSignal } from "solid-js";
import { render } from "solid-js/web";
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  LeftDock,
  LEFT_PANELS,
  leftPanelTitle,
  type LeftPanel,
} from "../LeftDock";
import { TapButton } from "../TapButton";
import { themeFor, uiScale } from "../theme";
import { ExplorerPanel } from "../ide/ExplorerPanel";
import { BranchesPanel } from "../ide/BranchesPanel";
import { LogPanel } from "../ide/LogPanel";
import { ProblemsPanel } from "../ide/ProblemsPanel";
import type { IdeSession } from "../ide/session";
import { FS_ENTRY_DIR, FS_ENTRY_FILE, LSP_SEVERITY_ERROR } from "@yas-run/core";
import { commitAssignment, editorAssignment } from "@yas-run/core/layout";
import { t } from "../i18n";

// jsdom has no canvas text metrics; the typography suite covers measurement.
vi.mock("@yas-run/core", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@yas-run/core")>()),
  measureCell: () => ({ w: 8, h: 17, pw: 8, ph: 17 }),
}));

let dispose: (() => void) | undefined;
afterEach(() => {
  dispose?.();
  document.body.replaceChildren();
  vi.unstubAllGlobals();
});

function touch(element: HTMLElement, type: string, x = 20) {
  const point = { identifier: -2, clientX: x, clientY: 20 };
  const event = new Event(type, { bubbles: true, cancelable: true });
  Object.defineProperties(event, {
    touches: {
      value: type === "touchend" || type === "touchcancel" ? [] : [point],
    },
    changedTouches: { value: [point] },
  });
  element.dispatchEvent(event);
  return event;
}

function geometry(element: HTMLElement) {
  vi.spyOn(element, "getBoundingClientRect").mockReturnValue(
    new DOMRect(0, 0, 200, 40),
  );
}

function tap(element: HTMLElement) {
  geometry(element);
  touch(element, "touchstart");
  touch(element, "touchend");
  element.dispatchEvent(
    new MouseEvent("click", {
      bubbles: true,
      cancelable: true,
      detail: 1,
      clientX: 20,
      clientY: 20,
    }),
  );
}

const panelProps = {
  theme: themeFor(true),
  scale: uiScale(14),
  fontFamily: "monospace",
  fontSize: 14,
};

function mount() {
  const [collapsed, setCollapsed] = createSignal<ReadonlySet<LeftPanel>>(
    new Set(LEFT_PANELS),
  );
  const extra = vi.fn();
  const toggle = vi.fn((panel: LeftPanel) =>
    setCollapsed((previous) => {
      const next = new Set(previous);
      if (!next.delete(panel)) next.add(panel);
      return next;
    }),
  );
  dispose = render(
    () => (
      <LeftDock
        collapsed={collapsed()}
        weights={{ explorer: 1, branches: 1, log: 1, problems: 1 }}
        theme={themeFor(true)}
        scale={uiScale(14)}
        width={260}
        isMobileTouch
        onResizeWidth={() => {}}
        onResizeWeight={() => {}}
        onToggleCollapse={toggle}
        renderBody={(panel) => <div data-panel-body={panel} />}
        renderExtra={() => <TapButton onClick={extra}>Extra</TapButton>}
      />
    ),
    document.body,
  );
  const header = (panel: LeftPanel) => {
    const label = Array.from(document.querySelectorAll("span")).find(
      (el) => el.textContent === leftPanelTitle(panel),
    )!;
    const element = label.parentElement!;
    geometry(element);
    return { element, label };
  };
  return { collapsed, toggle, extra, header };
}

describe("left dock touch activation", () => {
  it("toggles every section from touch events alone, once per tap", () => {
    const { collapsed, toggle, header } = mount();
    for (const panel of LEFT_PANELS) {
      const { element, label } = header(panel);
      touch(label, "touchstart");
      touch(label, "touchend");
      expect(collapsed().has(panel)).toBe(false);
      expect(
        document.querySelector(`[data-panel-body="${panel}"]`),
      ).not.toBeNull();
      element.dispatchEvent(
        new MouseEvent("click", {
          bubbles: true,
          cancelable: true,
          detail: 1,
          clientX: 20,
          clientY: 20,
        }),
      );
      expect(collapsed().has(panel)).toBe(false);
      touch(label, "touchstart");
      touch(label, "touchend");
      expect(collapsed().has(panel)).toBe(true);
    }
    expect(toggle).toHaveBeenCalledTimes(LEFT_PANELS.length * 2);
  });

  it("does not collapse a section when its extra control is tapped", () => {
    const { collapsed, toggle, extra, header } = mount();
    const { element } = header("explorer");
    element.click();
    toggle.mockClear();
    const button = element.querySelector("button")!;
    geometry(button);
    touch(button, "touchstart");
    touch(button, "touchend");
    expect(extra).toHaveBeenCalledOnce();
    expect(toggle).not.toHaveBeenCalled();
    expect(collapsed().has("explorer")).toBe(false);
  });

  it("opens files and expands directories without compatibility mouse events", () => {
    const onOpenTile = vi.fn();
    const toggleDir = vi.fn();
    const session = {
      connectionId: "test",
      root: () => "/repo",
      tree: () => [
        {
          relPath: "src",
          name: "src",
          type: FS_ENTRY_DIR,
          flags: 0,
          depth: 0,
          expanded: false,
          size: 0,
        },
        {
          relPath: "file.ts",
          name: "file.ts",
          type: FS_ENTRY_FILE,
          flags: 0,
          depth: 0,
          size: 123,
        },
      ],
      ensureTree: vi.fn(),
      fsError: () => null,
      gitState: () => null,
      gitHandle: () => null,
      fileAssignment: (path: string) =>
        editorAssignment("test", `/repo/${path}`),
      toggleDir,
    } as unknown as IdeSession;
    dispose = render(
      () => (
        <ExplorerPanel
          {...panelProps}
          session={session}
          onOpenTile={onOpenTile}
        />
      ),
      document.body,
    );
    tap(document.querySelector('[title="src"]')!);
    expect(toggleDir).toHaveBeenCalledExactlyOnceWith("src");
    tap(document.querySelector('[title="file.ts"]')!);
    expect(onOpenTile).toHaveBeenCalledExactlyOnceWith(
      editorAssignment("test", "/repo/file.ts"),
    );
  });

  it("selects branches and worktrees while keeping secondary actions independent", () => {
    const setLogSpec = vi.fn();
    const onOpenWorktree = vi.fn();
    const onOpenTerminalIn = vi.fn();
    const session = {
      ensureWorktrees: vi.fn(),
      noRepo: () => false,
      logSpec: () => "",
      setLogSpec,
      branches: () => ({
        local: [
          {
            ref: "refs/heads/main",
            label: "main",
            oid: "a".repeat(40),
            head: true,
          },
        ],
        remote: [],
        tags: [],
      }),
      worktrees: () => [
        {
          path: "/repo",
          name: "repo",
          branch: "refs/heads/main",
          branchLabel: "main",
          oid: "a".repeat(40),
          current: true,
        },
      ],
    } as unknown as IdeSession;
    dispose = render(
      () => (
        <BranchesPanel
          {...panelProps}
          session={session}
          onOpenWorktree={onOpenWorktree}
          onOpenTerminalIn={onOpenTerminalIn}
        />
      ),
      document.body,
    );
    tap(document.querySelector('[data-branch="refs/heads/main"]')!);
    expect(setLogSpec).toHaveBeenCalledExactlyOnceWith("refs/heads/main");
    const worktree = document.querySelector<HTMLElement>(
      '[data-worktree="/repo"]',
    )!;
    tap(worktree.querySelector("button")!);
    expect(onOpenTerminalIn).toHaveBeenCalledExactlyOnceWith("/repo");
    expect(onOpenWorktree).not.toHaveBeenCalled();
    tap(worktree);
    expect(onOpenWorktree).toHaveBeenCalledExactlyOnceWith("/repo");
  });

  it("opens commits and expands metadata without also opening the commit", () => {
    vi.stubGlobal(
      "ResizeObserver",
      class {
        observe() {}
        disconnect() {}
      },
    );
    const onOpenTile = vi.fn();
    const loadMoreLog = vi.fn();
    const oid = "a".repeat(40);
    const session = {
      connectionId: "test",
      repoWorkdir: () => "/repo",
      ensureLog: vi.fn(),
      commits: () => [
        {
          oid,
          parents: [],
          subject: "Test commit",
          author: "Test Author",
          time: 1n,
        },
      ],
      gitState: () => null,
      gitError: () => null,
      logSpec: () => "",
      logSpecError: () => null,
      hasMoreLog: () => true,
      loadMoreLog,
    } as unknown as IdeSession;
    dispose = render(
      () => (
        <LogPanel {...panelProps} session={session} onOpenTile={onOpenTile} />
      ),
      document.body,
    );
    const row = document.querySelector<HTMLElement>('[title="Test commit"]')!;
    tap(row.querySelector("button")!);
    expect(row.querySelector("button")!.textContent).toBe("Test Author");
    tap(row.querySelector(`[title="${t("log.toggleAbsoluteTime")}"]`)!);
    expect(onOpenTile).not.toHaveBeenCalled();
    tap(row);
    expect(onOpenTile).toHaveBeenCalledExactlyOnceWith(
      commitAssignment("test", oid, "/repo"),
    );
    const older = Array.from(
      document.querySelectorAll<HTMLElement>("[data-yas-tap]"),
    ).find((el) => el.textContent === t("log.loadOlder"))!;
    tap(older);
    expect(loadMoreLog).toHaveBeenCalledOnce();
  });

  it("opens a diagnostic from a touch-only tap", () => {
    const onOpenTile = vi.fn();
    const session = {
      connectionId: "test",
      ensureLsp: vi.fn(),
      lspVersion: () => 0,
      lspHandle: () => ({
        root: "/repo",
        state: { servers: new Map() },
        diags: {
          files: new Map([
            [
              "file.ts",
              {
                diags: [
                  {
                    line: 2,
                    col: 1,
                    severity: LSP_SEVERITY_ERROR,
                    msg: "Example problem",
                  },
                ],
              },
            ],
          ]),
        },
      }),
    } as unknown as IdeSession;
    dispose = render(
      () => (
        <ProblemsPanel
          {...panelProps}
          session={session}
          onOpenTile={onOpenTile}
        />
      ),
      document.body,
    );
    tap(document.querySelector('[title="file.ts:3:2"]')!);
    expect(onOpenTile).toHaveBeenCalledExactlyOnceWith(
      editorAssignment("test", "/repo/file.ts"),
    );
  });
});
