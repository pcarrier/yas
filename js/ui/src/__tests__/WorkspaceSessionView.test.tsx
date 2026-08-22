import { createSignal, onCleanup } from "solid-js";
import { render } from "solid-js/web";
import { afterEach, describe, expect, it, vi } from "vitest";
import { WorkspaceSessionView } from "../WorkspaceSessionView";
import type {
  WorkspaceSessionBinding,
  WorkspaceSessionController,
} from "../workspaceSession";
import { t } from "../i18n";

let dispose: (() => void) | undefined;
afterEach(() => {
  dispose?.();
  document.body.replaceChildren();
});

function mount(managed = true) {
  const [selected, setSelected] = createSignal<WorkspaceSessionBinding | null>(
    null,
  );
  const [loading, setLoading] = createSignal(true);
  const [error, setError] = createSignal<string | null>(null);
  const [managerOpen, setManagerOpen] = createSignal(false);
  const retry = vi.fn(async () => {});
  const controller = {
    loading,
    error,
    managerOpen,
    current: () => null,
    sessions: () => [],
    attachedSessions: () => [],
    attachedSessionIds: () => [],
    warnings: () => [],
    openManager: () => setManagerOpen(true),
    closeManager: () => setManagerOpen(false),
    retry,
  } as unknown as WorkspaceSessionController;
  const mounted = vi.fn();
  const unmounted = vi.fn();
  dispose = render(
    () => (
      <WorkspaceSessionView
        session={managed ? selected : undefined}
        controller={controller}
      >
        {(session) => {
          mounted(session);
          onCleanup(() => unmounted(session));
          return (
            <div data-workspace-screen>{session?.id ?? "local fallback"}</div>
          );
        }}
      </WorkspaceSessionView>
    ),
    document.body,
  );
  return { setSelected, setLoading, setError, mounted, unmounted, retry };
}

describe("workspace session rendering", () => {
  it("waits for the saved binding before mounting panes", () => {
    const state = mount();
    expect(state.mounted).not.toHaveBeenCalled();
    expect(document.querySelector("[data-workspace-screen]")).toBeNull();
    expect(document.querySelector('[aria-busy="true"]')).not.toBeNull();

    const saved = { id: "saved-layout" } as WorkspaceSessionBinding;
    state.setSelected(saved);
    expect(state.mounted).toHaveBeenCalledExactlyOnceWith(saved);
    expect(document.querySelector("[data-workspace-screen]")?.textContent).toBe(
      "saved-layout",
    );
    const screen = document.querySelector("[data-workspace-screen]");
    state.setLoading(false);
    expect(document.querySelector("[data-workspace-screen]")).toBe(screen);
    expect(state.unmounted).not.toHaveBeenCalled();
  });

  it("leaves an empty attachment list without an unbound workspace", () => {
    const state = mount();
    state.setLoading(false);
    expect(state.mounted).not.toHaveBeenCalled();
    expect(document.querySelector('[aria-busy="true"]')).toBeNull();
    expect(
      document.querySelector('button[aria-label="Open workspace manager"]'),
    ).not.toBeNull();
  });

  it("keeps store failures recoverable without mounting fallback panes", () => {
    const state = mount();
    state.setError("Cannot load workspace");
    state.setLoading(false);
    expect(state.mounted).not.toHaveBeenCalled();
    document
      .querySelector<HTMLButtonElement>(
        'button[aria-label^="Open workspace manager"]',
      )!
      .click();
    expect(document.querySelector('[role="alert"]')?.textContent).toBe(
      "Cannot load workspace",
    );
    const retry = [...document.querySelectorAll("button")].find(
      (button) => button.textContent === t("sessions.retry"),
    )!;
    expect(retry.textContent).toBe("Retry");
    retry.click();
    expect(state.retry).toHaveBeenCalledOnce();
  });

  it("unmounts detached panes and mounts the next saved workspace directly", () => {
    const state = mount();
    const first = { id: "first" } as WorkspaceSessionBinding;
    const second = { id: "second" } as WorkspaceSessionBinding;
    state.setSelected(first);
    state.setLoading(false);
    state.setSelected(null);
    expect(state.unmounted).toHaveBeenCalledExactlyOnceWith(first);
    expect(document.querySelector("[data-workspace-screen]")).toBeNull();
    state.setSelected(second);
    expect(state.mounted.mock.calls).toEqual([[first], [second]]);
  });

  it("mounts unmanaged embeds immediately", () => {
    const state = mount(false);
    expect(state.mounted).toHaveBeenCalledExactlyOnceWith(undefined);
    expect(document.querySelector("[data-workspace-screen]")?.textContent).toBe(
      "local fallback",
    );
  });
});
