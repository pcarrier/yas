/**
 * Which left-dock sections are folded, and what a click on a header means.
 *
 * Two independent reasons a section can be closed: the user collapsed it or
 * it does not apply to the current root — a commit log over a directory that
 * is not a repository has nothing to say, so it folds itself away. The distinction
 * matters at both ends: an auto-fold must not become a saved collapse (the
 * next root probably is a repo), and clicking an auto-folded header must open
 * it rather than record a collapse it already looks like it has.
 */

import {
  batch,
  createEffect,
  createMemo,
  createSignal,
  type Accessor,
} from "solid-js";

export type LeftPanel = "explorer" | "branches" | "log" | "problems";

/** Fixed section order in the accordion. Branches sits above the log because
 *  it is what retargets the log: the
 *  cause reads above its effect. */
export const LEFT_PANELS: LeftPanel[] = [
  "explorer",
  "branches",
  "log",
  "problems",
];

const isGitPanel = (panel: LeftPanel) =>
  panel === "branches" || panel === "log";

/** Git sections follow the root rather than inheriting a global collapse.
 * This also repairs saved workspaces whose empty expandedSections list used
 * to seed every section as explicitly collapsed. */
function persistentCollapsed(
  collapsed: ReadonlySet<LeftPanel>,
): Set<LeftPanel> {
  return new Set([...collapsed].filter((panel) => !isGitPanel(panel)));
}

export function createDockSections(options: {
  initialCollapsed: ReadonlySet<LeftPanel>;
  context: Accessor<string | null>;
  inapplicable: Accessor<ReadonlySet<LeftPanel>>;
}) {
  const [userCollapsed, setUserCollapsed] = createSignal(
    persistentCollapsed(options.initialCollapsed),
  );
  const [overridden, setOverridden] = createSignal<ReadonlySet<LeftPanel>>(
    new Set(),
  );
  let previousContext: string | null | undefined;
  let previousGitFolds = "";
  createEffect(() => {
    const context = options.context();
    const inapplicable = options.inapplicable();
    const gitFolds = LEFT_PANELS.filter(
      (panel) => isGitPanel(panel) && inapplicable.has(panel),
    ).join(",");
    const changed =
      context !== previousContext || gitFolds !== previousGitFolds;
    previousContext = context;
    previousGitFolds = gitFolds;
    batch(() => {
      if (changed)
        setUserCollapsed((cur) => {
          const next = persistentCollapsed(cur);
          return next.size === cur.size ? cur : next;
        });
      setOverridden((cur) => {
        const next = liveOverrides(cur, inapplicable);
        // A manual Git toggle applies only to this root and its settled
        // repository state. Entering/leaving a repo restores automatic folds.
        if (changed)
          for (const panel of next) if (isGitPanel(panel)) next.delete(panel);
        return next.size === cur.size ? cur : next;
      });
    });
  });
  return {
    collapsed: createMemo(() =>
      foldedSections(userCollapsed(), options.inapplicable(), overridden()),
    ),
    // Persist only global choices. Git choices are transient context overrides,
    // not defaults for every repository subsequently visited or reloaded.
    persistedCollapsed: createMemo(() => persistentCollapsed(userCollapsed())),
    toggle(panel: LeftPanel) {
      const next = toggleSection(
        panel,
        userCollapsed(),
        options.inapplicable(),
        overridden(),
      );
      batch(() => {
        setUserCollapsed(next.userCollapsed);
        setOverridden(next.overridden);
      });
    },
    expand(panel: LeftPanel) {
      batch(() => {
        setUserCollapsed((cur) => {
          const next = new Set(cur);
          next.delete(panel);
          return next;
        });
        if (options.inapplicable().has(panel))
          setOverridden((cur) => new Set(cur).add(panel));
      });
    },
  };
}

/**
 * The set LeftDock renders as collapsed: everything the user collapsed, plus
 * the inapplicable sections they have not overridden.
 */
export function foldedSections(
  userCollapsed: ReadonlySet<LeftPanel>,
  inapplicable: ReadonlySet<LeftPanel>,
  overridden: ReadonlySet<LeftPanel>,
): Set<LeftPanel> {
  const out = new Set(userCollapsed);
  for (const panel of inapplicable) if (!overridden.has(panel)) out.add(panel);
  return out;
}

/**
 * What toggling `panel`'s header changes. Auto-folded sections toggle the
 * override (a one-off "show it anyway", not a preference); everything else
 * toggles the manual collapse. The controller decides which choices persist.
 */
export function toggleSection(
  panel: LeftPanel,
  userCollapsed: ReadonlySet<LeftPanel>,
  inapplicable: ReadonlySet<LeftPanel>,
  overridden: ReadonlySet<LeftPanel>,
): { userCollapsed: Set<LeftPanel>; overridden: Set<LeftPanel> } {
  const nextCollapsed = new Set(userCollapsed);
  const nextOverridden = new Set(overridden);
  if (inapplicable.has(panel) && !userCollapsed.has(panel)) {
    if (nextOverridden.has(panel)) nextOverridden.delete(panel);
    else nextOverridden.add(panel);
  } else if (nextCollapsed.has(panel)) nextCollapsed.delete(panel);
  else nextCollapsed.add(panel);
  return { userCollapsed: nextCollapsed, overridden: nextOverridden };
}

/**
 * Overrides worth keeping: one lapses as soon as its section applies again, so
 * the next root without a repository folds the log afresh rather than
 * inheriting a decision made about a different root.
 */
export function liveOverrides(
  overridden: ReadonlySet<LeftPanel>,
  inapplicable: ReadonlySet<LeftPanel>,
): Set<LeftPanel> {
  return new Set([...overridden].filter((panel) => inapplicable.has(panel)));
}
