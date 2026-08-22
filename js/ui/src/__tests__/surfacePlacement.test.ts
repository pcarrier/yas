import { describe, expect, it } from "vitest";
import {
  managedWindowTarget,
  observeTopLevelSurface,
  pendingSurfacePlacementIsRetired,
  restoredSurfaceAssignments,
  shouldPlaceObservedSurface,
  surfacePlacementIdentity,
} from "../layout/surfacePlacement";
import { surfaceWorkspaceRef, tabWorkspaceRef } from "../layout/store";
import { surfaceAssignment } from "@yas-run/core/layout";

describe("surface placement", () => {
  it("uses an existing empty pane before splitting", () => {
    expect(
      managedWindowTarget(["0", "1"], { "0": "terminal", "1": null }, "0"),
    ).toEqual({ paneId: "1", split: false });
  });

  it("splits the focused pane instead of replacing its occupant", () => {
    expect(
      managedWindowTarget(["0", "1"], { "0": "first", "1": "second" }, "1"),
    ).toEqual({ paneId: "1", split: true });
  });

  it("does not confuse a reused surface handle after a server restart", () => {
    const surface = { connectionId: "local", surfaceId: 1n };
    expect(surfacePlacementIdentity(surface, 10n)).not.toBe(
      surfacePlacementIdentity(surface, 11n),
    );
  });

  it("does not lose a child that later becomes a top-level window", () => {
    const known = new Set<string>();
    expect(observeTopLevelSurface(known, "dev:10:7", false)).toBe(false);
    expect(known.size).toBe(0);
    expect(observeTopLevelSurface(known, "dev:10:7", true)).toBe(true);
    expect(observeTopLevelSurface(known, "dev:10:7", true)).toBe(false);
  });

  it("keeps unassigned surfaces from a restored catalogue parked", () => {
    const initialCataloguesComplete = new Set<string>();
    const disposition = (explicitlyRestored: boolean) =>
      shouldPlaceObservedSurface({
        restoringSavedLayout: true,
        initialCataloguesComplete,
        connectionId: "dev",
        explicitlyRestored,
      });

    expect(disposition(false)).toBe(false);
    expect(disposition(true)).toBe(true);
    initialCataloguesComplete.add("dev");
    expect(disposition(false)).toBe(true);
    expect(
      shouldPlaceObservedSurface({
        restoringSavedLayout: false,
        initialCataloguesComplete: new Set(),
        connectionId: "dev",
        explicitlyRestored: false,
      }),
    ).toBe(true);
  });

  it("keeps a pending window through a non-ready catalogue reset", () => {
    const assignment = surfaceAssignment("dev", 7n);
    const identity = "dev:10:7";
    const known = new Set<string>();
    const pending = new Set<string>();

    expect(observeTopLevelSurface(known, identity, true)).toBe(true);
    pending.add(assignment);

    // Family reconfiguration first publishes an empty catalogue while the
    // connection is non-ready. That is not evidence the window was destroyed.
    const retiredDuringReset = pendingSurfacePlacementIsRetired(
      assignment,
      new Set(),
      new Set(),
    );
    expect(retiredDuringReset).toBe(false);
    if (retiredDuringReset) pending.delete(assignment);

    // The replacement catalogue carries the same boot-scoped identity, so it
    // is not observed as new; the original pending request must still own it.
    expect(observeTopLevelSurface(known, identity, true)).toBe(false);
    expect(pending.has(assignment)).toBe(true);
    expect(
      pendingSurfacePlacementIsRetired(
        assignment,
        new Set([assignment]),
        new Set(["dev"]),
      ),
    ).toBe(false);
  });

  it("retires a missing pending window after its catalogue is authoritative", () => {
    const assignment = surfaceAssignment("dev", 7n);
    expect(
      pendingSurfacePlacementIsRetired(assignment, new Set(), new Set(["dev"])),
    ).toBe(true);
  });

  it("distinguishes restored surfaces from unclaimed live windows", () => {
    expect(
      restoredSurfaceAssignments({
        "0": surfaceWorkspaceRef("relay:dev", 7n),
        "1": tabWorkspaceRef("relay:dev", "editor"),
        "2": "invalid",
      }),
    ).toEqual(new Set([surfaceAssignment("relay:dev", 7n)]));
  });
});
