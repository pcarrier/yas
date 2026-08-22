import { describe, expect, it, vi } from "vitest";
import type { YasRelayRoute, YasWorkspaceConnection } from "@yas-run/core";
import { RelayConnectionCache } from "../relayTransportCache";
import {
  mergeWorkspaceSessionRemotes,
  reconcileWorkspaceSessionRelayConnections,
  workspaceSessionRemoteRows,
} from "../workspaceSessionRemotes";

function route(name: string): YasRelayRoute {
  return {
    handle: 7n,
    generation: 1n,
    name,
    label: name.toUpperCase(),
    availability: 1,
    transportHint: 0,
    flags: 0,
    description: "",
    extensions: [],
  };
}

describe("workspace remote connection lifecycle", () => {
  it("retains disabled catalogue entries outside the Relay projection", () => {
    expect(
      mergeWorkspaceSessionRemotes(
        [route("hound")],
        [
          { name: "hound", disabled: false },
          { name: "prod", disabled: true },
        ],
      ),
    ).toEqual([
      { name: "local", uri: "home server", disabled: false },
      { name: "hound", uri: "home-server relay", disabled: false },
      { name: "prod", uri: "home-server relay", disabled: true },
    ]);
  });

  it("constructs only active routes, evicts on detach, and recreates on attach", () => {
    const cache = new RelayConnectionCache(() => {});
    const closes: Array<ReturnType<typeof vi.fn>> = [];
    const create = vi.fn(() => {
      const close = vi.fn();
      closes.push(close);
      return {
        close,
        dispose: vi.fn(),
        transport: {
          addEventListener: vi.fn(),
          removeEventListener: vi.fn(),
        },
      } as unknown as YasWorkspaceConnection;
    });
    const routes = [route("hound")];
    const saved = ["hound", "temporarily-missing"];

    expect(
      reconcileWorkspaceSessionRelayConnections(routes, [], cache, create),
    ).toEqual([]);
    expect(create).not.toHaveBeenCalled();
    expect(cache.stats().entries).toBe(0);

    expect(
      reconcileWorkspaceSessionRelayConnections(routes, saved, cache, create),
    ).toHaveLength(1);
    expect(create).toHaveBeenCalledTimes(1);
    expect(cache.stats().entries).toBe(1);

    expect(
      reconcileWorkspaceSessionRelayConnections(routes, [], cache, create),
    ).toEqual([]);
    expect(closes[0]).toHaveBeenCalledTimes(1);
    expect(cache.stats().entries).toBe(0);

    expect(
      reconcileWorkspaceSessionRelayConnections(routes, saved, cache, create),
    ).toHaveLength(1);
    expect(create).toHaveBeenCalledTimes(2);
    expect(cache.stats().entries).toBe(1);

    // Selection is not catalogue intersection: the missing saved route stays
    // durable and visible so it can reconnect if its route returns.
    expect(saved).toEqual(["hound", "temporarily-missing"]);
    expect(
      workspaceSessionRemoteRows(
        [{ name: "hound", label: "HOUND", available: true }],
        saved,
        "local",
      ),
    ).toEqual([
      { name: "local", label: "local", available: true },
      { name: "hound", label: "HOUND", available: true },
      {
        name: "temporarily-missing",
        label: "temporarily-missing",
        available: false,
      },
    ]);
  });
});
