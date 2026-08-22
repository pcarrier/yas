import { describe, expect, it } from "vitest";
import type { YasSession } from "@yas-run/core";
import {
  currentSessionForPty,
  currentSourceSessionForPty,
  isSourceTerminalUnavailableError,
  sourceSessionCanResolveCwd,
} from "../ide/followTerminal";

/**
 * A dock session anchored on a terminal opens fs/git/lsp FROM_PTY: the server
 * resolves the root from that pty's live cwd. It names the pty by SessionId,
 * and SessionIds live for exactly one connection generation — every
 * re-establish marks the current sessions closed, mints new ids for the same
 * ptys, and prunes the superseded ones. The dock session is keyed by pty and
 * stays warm across all of that, so it has to re-resolve.
 */
const session = (
  id: string,
  ptyId: bigint,
  state: YasSession["state"] = "active",
  connectionId = "local",
): YasSession => ({
  id,
  connectionId,
  ptyId,
  tag: "",
  title: null,
  usedRows: 0,
  command: null,
  state,
  exitStatus: null,
});

describe("follow-terminal source resolution", () => {
  it("ignores same-pty sessions on other connections", () => {
    // pty ids are per-connection; following one across connections would open
    // an unrelated terminal's cwd.
    const other = [session("remote:1", 7n, "active", "remote")];
    expect(currentSessionForPty(other, "local", 7n, "local:1")).toBe("local:1");
  });

  it("follows the pty to the session id minted by a reconnect", () => {
    // Mid-reconnect: HELLO closed generation 1, LIST added generation 2, and
    // the prune has not run yet.
    const midReconnect = [
      session("local:1", 7n, "closed"),
      session("local:2", 7n),
    ];
    expect(currentSessionForPty(midReconnect, "local", 7n, "local:1")).toBe(
      "local:2",
    );
    // After the prune only the live one is left.
    expect(
      currentSessionForPty([session("local:2", 7n)], "local", 7n, "local:1"),
    ).toBe("local:2");
  });

  it("takes the newest known session while every one is closed", () => {
    // The window between HELLO (all closed) and LIST: the newest closed
    // session is still the one the connection can resolve to a pty, so an
    // open issued here must use it rather than a pruned older id.
    const closed = [
      session("local:1", 7n, "closed"),
      session("local:2", 7n, "closed"),
    ];
    expect(currentSessionForPty(closed, "local", 7n, "local:1")).toBe(
      "local:2",
    );
    const source = currentSourceSessionForPty(closed, "local", 7n);
    expect(sourceSessionCanResolveCwd(source, false)).toBe(true);
    // Once the replacement LIST/READY completed, a still-closed session was
    // not restored by the server and can no longer anchor an open.
    expect(sourceSessionCanResolveCwd(source, true)).toBe(false);
  });

  it("falls back when the pty is gone", () => {
    // The terminal itself exited: nothing to follow, so the caller keeps its
    // own id. The server refuses that safely instead of silently rebasing onto
    // its cwd; the UI classifies the refusal as a lifecycle race below.
    expect(currentSessionForPty([], "local", 7n, "local:1")).toBe("local:1");
    expect(
      currentSessionForPty(
        [session("local:1", 7n, "exited")],
        "local",
        7n,
        "fallback",
      ),
    ).toBe("fallback");
    expect(
      currentSourceSessionForPty(
        [session("local:1", 7n, "closed"), session("local:2", 7n, "exited")],
        "local",
        7n,
      ),
    ).toBeNull();
  });

  it("recognizes the source-cwd race without hiding other sync errors", () => {
    expect(
      isSourceTerminalUnavailableError(
        new Error(
          "Sync failed: not found: source terminal has no working directory",
        ),
      ),
    ).toBe(true);
    expect(
      isSourceTerminalUnavailableError(
        new Error("Sync failed: not found: project directory was removed"),
      ),
    ).toBe(false);
  });
});
