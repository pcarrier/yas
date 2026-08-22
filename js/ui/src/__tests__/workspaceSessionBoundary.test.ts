import { createSignal } from "solid-js";
import { describe, expect, it } from "vitest";
import type { WorkspaceSessionBinding } from "../workspaceSession";
import { workspaceSessionBoundary } from "../workspaceSessionBoundary";

function binding(id: string): WorkspaceSessionBinding {
  return { id } as WorkspaceSessionBinding;
}

describe("workspace session screen boundary", () => {
  it("keeps unmanaged embed workspaces visible when the prop is omitted", () => {
    const boundary = workspaceSessionBoundary(undefined);

    expect(boundary.managed).toBe(false);
    expect(boundary.current()).toBeNull();
  });

  it("distinguishes a managed manager-only state and reacts to selection", () => {
    const [selected, setSelected] =
      createSignal<WorkspaceSessionBinding | null>(null);
    const boundary = workspaceSessionBoundary(selected);

    expect(boundary.managed).toBe(true);
    expect(boundary.current()).toBeNull();
    const next = binding("session-a");
    setSelected(next);
    expect(boundary.current()).toBe(next);
  });

  it("supports the static binding used by standalone managed callers", () => {
    const selected = binding("session-a");
    const boundary = workspaceSessionBoundary(selected);

    expect(boundary.managed).toBe(true);
    expect(boundary.current()).toBe(selected);
  });
});
