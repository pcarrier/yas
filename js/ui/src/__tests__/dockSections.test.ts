import { afterEach, describe, expect, it } from "vitest";
import { batch, createRoot, createSignal } from "solid-js";
import { createDefaultStoredWorkspaceSession } from "@yas-run/core";
import type { LeftPanel } from "../LeftDock";
import {
  createDockSections,
  LEFT_PANELS,
  foldedSections,
  liveOverrides,
  toggleSection,
} from "../dockSections";
import { preferredCollapsedSections } from "../storage";

const set = (...panels: LeftPanel[]) => new Set<LeftPanel>(panels);
const none = new Set<LeftPanel>();

describe("foldedSections", () => {
  it("folds what does not apply on top of what the user collapsed", () => {
    expect(foldedSections(set("problems"), set("log"), none)).toEqual(
      set("problems", "log"),
    );
  });

  it("leaves an overridden section open", () => {
    // The user opened the auto-folded log to read why it is empty.
    expect(foldedSections(none, set("log"), set("log"))).toEqual(none);
  });

  it("keeps a user collapse even when the section is overridden", () => {
    // The override only answers the auto-fold; a deliberate collapse stands.
    expect(foldedSections(set("log"), set("log"), set("log"))).toEqual(
      set("log"),
    );
  });
});

describe("toggleSection", () => {
  it("records a collapse for a section that applies", () => {
    const next = toggleSection("log", none, none, none);
    expect(next.userCollapsed).toEqual(set("log"));
    expect(next.overridden).toEqual(none);
  });

  it("opens an auto-folded section without touching the preference", () => {
    // Otherwise the click would store a collapse the section already looks
    // like it has, and the header would stay shut.
    const next = toggleSection("log", none, set("log"), none);
    expect(next.userCollapsed).toEqual(none);
    expect(next.overridden).toEqual(set("log"));
    expect(foldedSections(none, set("log"), next.overridden)).toEqual(none);
  });

  it("re-folds an overridden section on the next click", () => {
    const next = toggleSection("log", none, set("log"), set("log"));
    expect(next.overridden).toEqual(none);
    expect(next.userCollapsed).toEqual(none);
  });

  it("un-collapses a user-collapsed section that also does not apply", () => {
    // The preference is what is showing, so the click answers that first;
    // the auto-fold then keeps it shut until it is clicked again.
    const next = toggleSection("log", set("log"), set("log"), none);
    expect(next.userCollapsed).toEqual(none);
    expect(next.overridden).toEqual(none);
    expect(
      foldedSections(next.userCollapsed, set("log"), next.overridden),
    ).toEqual(set("log"));
  });
});

describe("liveOverrides", () => {
  it("drops overrides once the section applies again", () => {
    // Moving to a root that has a repository: the log must come back on its
    // own, and the next root without one must fold it afresh.
    expect(liveOverrides(set("log"), none)).toEqual(none);
    expect(liveOverrides(set("log"), set("log"))).toEqual(set("log"));
  });
});

describe("Git sections follow the repository", () => {
  let dispose: (() => void) | undefined;
  afterEach(() => {
    dispose?.();
    localStorage.clear();
  });

  const firstRun = () => new Set(preferredCollapsedSections() as LeftPanel[]);
  const mount = (
    initialCollapsed = firstRun(),
    initialRoot: string | null = "/plain",
    noRepo = true,
  ) =>
    createRoot((cleanup) => {
      dispose = cleanup;
      const [context, setContext] = createSignal(initialRoot);
      const [inapplicable, setInapplicable] = createSignal<
        ReadonlySet<LeftPanel>
      >(noRepo ? set("branches", "log") : none);
      const dock = createDockSections({
        initialCollapsed,
        context,
        inapplicable,
      });
      return {
        ...dock,
        setInapplicable,
        visit(root: string | null, noRepo: boolean) {
          batch(() => {
            setContext(root);
            setInapplicable(noRepo ? set("branches", "log") : none);
          });
        },
      };
    });

  const expectGitFolded = (
    collapsed: ReadonlySet<LeftPanel>,
    folded: boolean,
  ) => {
    expect(collapsed.has("branches")).toBe(folded);
    expect(collapsed.has("log")).toBe(folded);
  };

  it("opens both Git sections from a newly created workspace's empty expandedSections", () => {
    const stored = createDefaultStoredWorkspaceSession({
      id: "00000000-0000-4000-8000-000000000001",
    });
    const initial = new Set(
      LEFT_PANELS.filter(
        (panel) => !stored.workspace.panels.expandedSections.includes(panel),
      ),
    );
    expectGitFolded(initial, true);
    const dock = mount(initial, "/repo", false);
    expectGitFolded(dock.collapsed(), false);
    // Non-Git choices remain exactly as stored.
    expect(dock.collapsed().has("explorer")).toBe(true);
    expect(dock.collapsed().has("problems")).toBe(true);
  });

  it("folds outside repositories, opens on entry, and folds again on exit", () => {
    const dock = mount();
    expectGitFolded(dock.collapsed(), true);
    dock.visit("/repo", false);
    expectGitFolded(dock.collapsed(), false);
    dock.visit("/plain", true);
    expectGitFolded(dock.collapsed(), true);
    dock.visit("/other-repo", false);
    expectGitFolded(dock.collapsed(), false);
  });

  it("keeps manual collapses only for the current repository", () => {
    const dock = mount(firstRun(), "/repo", false);
    dock.toggle("branches");
    dock.toggle("log");
    expectGitFolded(dock.collapsed(), true);
    // Unrelated status refreshes must not undo the user's action.
    dock.setInapplicable(new Set<LeftPanel>());
    expectGitFolded(dock.collapsed(), true);
    // Local-storage and workspace snapshots must not seed a global collapse.
    expectGitFolded(dock.persistedCollapsed(), false);
    dock.visit("/other-repo", false);
    expectGitFolded(dock.collapsed(), false);
    dock.visit("/repo", false);
    expectGitFolded(dock.collapsed(), false);
  });

  it("does not carry a manually expanded empty section to another plain directory", () => {
    const dock = mount();
    dock.toggle("branches");
    dock.toggle("log");
    expectGitFolded(dock.collapsed(), false);
    dock.visit("/other-plain", true);
    expectGitFolded(dock.collapsed(), true);
  });

  it("resets overrides when Git availability changes without changing the root", () => {
    const dock = mount(firstRun(), "/repo", false);
    dock.toggle("branches");
    dock.toggle("log");
    dock.setInapplicable(set("branches", "log"));
    expectGitFolded(dock.collapsed(), true);
    dock.setInapplicable(none);
    expectGitFolded(dock.collapsed(), false);
  });

  it("retains global Files/Problems preferences and allows explicit expansion", () => {
    const dock = mount();
    expect(dock.collapsed().has("problems")).toBe(true);
    dock.toggle("explorer");
    dock.visit("/repo", false);
    expect(dock.persistedCollapsed()).toEqual(set("explorer", "problems"));
    dock.visit("/plain", true);
    dock.expand("log");
    expect(dock.collapsed().has("log")).toBe(false);
    expect(dock.collapsed().has("branches")).toBe(true);
    expect(dock.persistedCollapsed()).toEqual(set("explorer", "problems"));
  });

  it("folds without a selected root and opens once a repository is selected", () => {
    const dock = mount(new Set(LEFT_PANELS), null);
    expectGitFolded(dock.collapsed(), true);
    dock.visit("/repo", false);
    expectGitFolded(dock.collapsed(), false);
  });
});
