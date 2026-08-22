import { describe, expect, it, vi } from "vitest";
import {
  claimHmrLease,
  closeTransportBundle,
  createHmrConnectionSlot,
  deferHmrRelease,
  type HmrLeaseState,
} from "../hmrLease";

describe("HMR state leases", () => {
  it("retires ping timers on repeated hot replacements without component cleanup", () => {
    vi.useFakeTimers();
    try {
      const pings = [vi.fn(), vi.fn(), vi.fn()];
      const slot = createHmrConnectionSlot();
      const connections = pings.map((ping) => {
        const timer = setInterval(ping, 1_000);
        return { close: vi.fn(), dispose: vi.fn(() => clearInterval(timer)) };
      });
      const staleCleanups = connections.map((connection) =>
        slot.replace(connection),
      );
      vi.advanceTimersByTime(5_000);
      expect(pings[0]).not.toHaveBeenCalled();
      expect(pings[1]).not.toHaveBeenCalled();
      expect(pings[2]).toHaveBeenCalledTimes(5);
      slot.close();
      staleCleanups.forEach((cleanup) => cleanup());
      vi.advanceTimersByTime(5_000);
      expect(pings[2]).toHaveBeenCalledTimes(5);
      for (const connection of connections) {
        expect(connection.close).toHaveBeenCalledOnce();
        expect(connection.dispose).toHaveBeenCalledOnce();
      }
    } finally {
      vi.useRealTimers();
    }
  });

  it("closes connections from late mounts of a disposed module", () => {
    const generation = createHmrConnectionSlot();
    generation.close();
    const lateConnection = { close: vi.fn() };
    const cleanup = generation.replace(lateConnection);
    expect(lateConnection.close).toHaveBeenCalledOnce();
    cleanup();
    generation.close();
    expect(lateConnection.close).toHaveBeenCalledOnce();
  });

  it("keeps the latest mount when closing a connection reenters replacement", () => {
    const generation = createHmrConnectionSlot();
    const latest = { close: vi.fn() };
    const previous = {
      close: vi.fn(() => generation.replace(latest)),
    };
    const superseded = { close: vi.fn() };
    generation.replace(previous);
    const staleCleanup = generation.replace(superseded);
    expect(previous.close).toHaveBeenCalledOnce();
    expect(superseded.close).toHaveBeenCalledOnce();
    staleCleanup();
    expect(latest.close).not.toHaveBeenCalled();
    generation.close();
    expect(latest.close).toHaveBeenCalledOnce();
  });

  it("closes abandoned connections on HMR without closing a replacement", () => {
    const oldGeneration = createHmrConnectionSlot();
    const oldConnection = { close: vi.fn() };
    const staleCleanup = oldGeneration.replace(oldConnection);

    // The module is disposed even if Solid never visits the old component.
    oldGeneration.close();
    const nextGeneration = createHmrConnectionSlot();
    const nextConnection = { close: vi.fn() };
    const nextCleanup = nextGeneration.replace(nextConnection);
    staleCleanup();
    oldGeneration.close();
    expect(oldConnection.close).toHaveBeenCalledOnce();
    expect(nextConnection.close).not.toHaveBeenCalled();

    nextCleanup();
    nextGeneration.close();
    expect(nextConnection.close).toHaveBeenCalledOnce();
  });

  it("forgets normally unmounted connections before module disposal", () => {
    const generation = createHmrConnectionSlot();
    const unmounted = { close: vi.fn() };
    const mounted = { close: vi.fn() };
    generation.replace(unmounted)();
    generation.replace(mounted);
    generation.close();
    expect(unmounted.close).toHaveBeenCalledOnce();
    expect(mounted.close).toHaveBeenCalledOnce();
  });

  it("closes the previous hot mount before its replacement connects", () => {
    const generation = createHmrConnectionSlot();
    const oldConnection = { close: vi.fn() };
    const nextConnection = { close: vi.fn() };
    const staleCleanup = generation.replace(oldConnection);
    generation.replace(nextConnection);
    expect(oldConnection.close).toHaveBeenCalledOnce();
    staleCleanup();
    expect(nextConnection.close).not.toHaveBeenCalled();
    generation.close();
    expect(nextConnection.close).toHaveBeenCalledOnce();
  });

  it("closes the mux and every retained upstream channel", () => {
    const mux = { close: vi.fn() };
    const channels = [
      { close: vi.fn() },
      { close: vi.fn() },
      { close: vi.fn() },
    ];
    const cache = new Map(
      channels.map((transport, index) => [`remote-${index}`, { transport }]),
    );

    closeTransportBundle(mux, cache);

    expect(mux.close).toHaveBeenCalledOnce();
    for (const channel of channels) {
      expect(channel.close).toHaveBeenCalledOnce();
    }
    expect(cache.size).toBe(0);
  });

  it("releases state after a real development-mode unmount", () => {
    vi.useFakeTimers();
    try {
      const state: HmrLeaseState = {};
      const owner = {};
      let current: HmrLeaseState | null = state;
      const release = vi.fn();
      claimHmrLease(state, owner);

      deferHmrRelease(
        state,
        owner,
        () => current === state,
        release,
        () => {
          current = null;
        },
      );
      vi.runAllTimers();

      expect(release).toHaveBeenCalledOnce();
      expect(current).toBeNull();
    } finally {
      vi.useRealTimers();
    }
  });

  it("keeps state when the replacement hot mount claims it", () => {
    vi.useFakeTimers();
    try {
      const state: HmrLeaseState = {};
      const oldOwner = {};
      const newOwner = {};
      const release = vi.fn();
      claimHmrLease(state, oldOwner);

      deferHmrRelease(
        state,
        oldOwner,
        () => true,
        release,
        () => {},
      );
      claimHmrLease(state, newOwner);
      vi.runAllTimers();

      expect(release).not.toHaveBeenCalled();
      expect(state.hmrLeaseOwner).toBe(newOwner);
    } finally {
      vi.useRealTimers();
    }
  });

  it("does not let a stale mount release replacement state", () => {
    vi.useFakeTimers();
    try {
      const state: HmrLeaseState = {};
      const oldOwner = {};
      const newOwner = {};
      const release = vi.fn();
      claimHmrLease(state, newOwner);

      deferHmrRelease(
        state,
        oldOwner,
        () => true,
        release,
        () => {},
      );
      vi.runAllTimers();

      expect(release).not.toHaveBeenCalled();
    } finally {
      vi.useRealTimers();
    }
  });
});
