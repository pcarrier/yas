import { describe, expect, it, vi } from "vitest";
import {
  consumePassphraseFromHash,
  normalizeWorkspaceSessionId,
  workspaceSessionHash,
  workspaceSessionIdFromHash,
  workspaceSessionRequestFromHash,
  workspaceSessionShareUrl,
  writeWorkspaceSessionUrl,
  storedWorkspaceSessionId,
  WORKSPACE_SESSION_STORAGE_KEY,
} from "../workspaceSessionUrl";

const ID = "123e4567-e89b-42d3-a456-426614174000";

describe("workspace session URL", () => {
  it("reads and canonicalizes UUID session IDs", () => {
    expect(workspaceSessionIdFromHash(`#session=${ID.toUpperCase()}`)).toBe(ID);
    expect(workspaceSessionIdFromHash(`#debug&session=${ID}`)).toBe(ID);
    expect(workspaceSessionIdFromHash("#session=not-a-session")).toBeNull();
    expect(normalizeWorkspaceSessionId(`${ID}x`)).toBeNull();
    expect(workspaceSessionRequestFromHash("#session=malformed")).toEqual({
      present: true,
      id: null,
    });
    expect(workspaceSessionRequestFromHash("#debug")).toEqual({
      present: false,
      id: null,
    });
  });

  it("emits only the canonical session field", () => {
    expect(workspaceSessionHash(ID)).toBe(`session=${ID}`);
    expect(() => workspaceSessionHash("../bad")).toThrow(
      "invalid workspace session ID",
    );
    expect(
      workspaceSessionShareUrl(
        { origin: "https://yas.example", pathname: "/ui" },
        ID,
      ),
    ).toBe(`https://yas.example/ui#session=${ID}`);
  });

  it("copies only the canonical session URL", () => {
    expect(
      workspaceSessionShareUrl(
        { origin: "https://yas.example", pathname: "/console" } as Location,
        ID,
      ),
    ).toBe(`https://yas.example/console#session=${ID}`);
  });

  it("removes all passphrase fields while retaining other fields exactly", () => {
    expect(
      consumePassphraseFromHash(
        `#session=${ID}&psk=hello%20world&debug&psk=ignored`,
      ),
    ).toEqual({
      passphrase: "hello world",
      hash: `session=${ID}&debug`,
      found: true,
    });
  });

  it("does not rewrite a fragment without a delivered credential", () => {
    expect(consumePassphraseFromHash(`#session=${ID}`)).toEqual({
      passphrase: null,
      hash: `session=${ID}`,
      found: false,
    });
  });

  it("records the session and leaves a clean address", () => {
    history.replaceState(null, "", `/app?ignored=1#session=${ID}&old`);
    const replace = vi.spyOn(history, "replaceState");
    const push = vi.spyOn(history, "pushState");

    // The id goes to storage; the fragment keeps only what was not ours.
    writeWorkspaceSessionUrl(ID, "replace");
    expect(localStorage.getItem(WORKSPACE_SESSION_STORAGE_KEY)).toBe(ID);
    expect(replace).toHaveBeenLastCalledWith(null, "", "/app#old");

    // Nothing left to strip: a second write touches storage and not history.
    writeWorkspaceSessionUrl(ID, "push");
    expect(push).not.toHaveBeenCalled();

    writeWorkspaceSessionUrl(null, "replace");
    expect(localStorage.getItem(WORKSPACE_SESSION_STORAGE_KEY)).toBeNull();
  });

  it("never leaves a first-contact passphrase in the address", () => {
    history.replaceState(null, "", `/app#psk=secret&session=${ID}`);
    writeWorkspaceSessionUrl(ID, "replace");
    expect(location.hash).toBe("");
  });

  it("reads back the session this device attached", () => {
    localStorage.setItem(WORKSPACE_SESSION_STORAGE_KEY, ID.toUpperCase());
    expect(storedWorkspaceSessionId()).toBe(ID);
    localStorage.setItem(WORKSPACE_SESSION_STORAGE_KEY, "not-a-uuid");
    expect(storedWorkspaceSessionId()).toBeNull();
  });
});
