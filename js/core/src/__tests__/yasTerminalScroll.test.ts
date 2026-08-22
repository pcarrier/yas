import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { YasNativeWorkspaceConnection } from "../YasNativeWorkspaceConnection";
import { YasTerminalSurface } from "../YasTerminalSurface";
import {
  YAS_TERMINAL_SCROLL_ABSOLUTE,
  YAS_TERMINAL_SCROLL_RELATIVE,
} from "../yas/generated";

vi.mock("../measure", () => ({
  measureCell: () => ({ w: 10, h: 20, pw: 20, ph: 40 }),
}));

describe("native terminal scroll replies", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.stubGlobal(
      "requestAnimationFrame",
      vi.fn(() => 1),
    );
  });
  afterEach(() => {
    vi.clearAllTimers();
    vi.useRealTimers();
    vi.unstubAllGlobals();
  });

  function rig(maximum = 100) {
    let serverOffset = 0;
    const replies: (() => void)[] = [];
    const scroll = vi.fn((_view: number, amount: bigint, mode: number) => {
      const requested =
        mode === YAS_TERMINAL_SCROLL_ABSOLUTE
          ? Number(amount)
          : serverOffset + Number(amount);
      serverOffset = Math.max(0, Math.min(maximum, requested));
      const applied = BigInt(serverOffset);
      return new Promise<bigint>((resolve) => {
        replies.push(() => resolve(applied));
      });
    });
    const state = {
      view: {
        result: { viewId: 7, maxInflightFrames: 4 },
        feedback: () => ({}),
      },
      pendingSequences: [],
      lastPresented: 0,
    };
    const connection = Object.create(
      YasNativeWorkspaceConnection.prototype,
    ) as YasNativeWorkspaceConnection;
    const views = new Map([[1n, state]]);
    Object.assign(connection, {
      sessions: new Map([["s1", { ptyId: 1n }]]),
      views,
      terminalClient: { scroll, input: vi.fn() },
      transport: { status: "connected" },
      scrollAnchorListeners: new Map(),
    });
    const anchor = vi.fn();
    connection.addScrollAnchorListener("s1", anchor);
    const answer = async () => {
      replies.shift()!();
      await Promise.resolve();
    };
    return {
      connection,
      views,
      scroll,
      anchor,
      answer,
      server: () => serverOffset,
    };
  }

  it.each([1, 2, 5])(
    "keeps a swipe monotonic with replies %i moves late",
    async (lag) => {
      const { connection, scroll, answer, server } = rig();
      const surface = new YasTerminalSurface({ sessionId: "s1" });
      const el = document.createElement("div");
      Object.assign(surface, {
        scrollEl: el,
        scrollSpacer: document.createElement("div"),
        terminal: { scrollback_lines: () => 100 },
        cell: { h: 10 },
        _yasConn: connection,
        _workspace: connection,
      });
      surface["setupScrollAnchorListener"]();
      surface["setupScrollSurface"]();
      for (let frame = 1; frame <= 18; frame++) {
        el.scrollTop = 1000 - frame * 20;
        el.dispatchEvent(new Event("scroll"));
        if (frame >= lag) await answer();
        expect(surface["scrollOffset"]).toBe(frame * 2);
      }
      for (let frame = 1; frame < lag; frame++) await answer();
      expect(scroll.mock.calls.map((call) => [call[1], call[2]])).toEqual(
        Array.from({ length: 18 }, () => [2n, YAS_TERMINAL_SCROLL_RELATIVE]),
      );
      expect(server()).toBe(36);
      expect(surface["scrollOffset"]).toBe(36);
    },
  );

  it("applies the latest server clamp without replaying older positions", async () => {
    const { connection, anchor, answer } = rig(80);
    connection.scrollSessionBy("s1", 50, 50);
    connection.scrollSessionBy("s1", 100, 50);
    await answer();
    expect(anchor).not.toHaveBeenCalled();
    await answer();
    expect(anchor).toHaveBeenCalledExactlyOnceWith(80);
  });

  it("does not replay a pending scroll after jumping to the live prompt", async () => {
    const { connection, anchor, answer } = rig();
    connection.scrollSessionBy("s1", 50, 50);
    connection.scrollSession("s1", 0);
    await answer();
    expect(anchor).not.toHaveBeenCalled();
    await answer();
    expect(anchor).not.toHaveBeenCalled();
  });

  it("discards replies from a view that has closed", async () => {
    const { connection, views, anchor, answer } = rig(80);
    connection.scrollSessionBy("s1", 100, 100);
    views.clear();
    await answer();
    expect(anchor).not.toHaveBeenCalled();
  });

  it("discards pending scroll corrections when typing returns to live output", async () => {
    const { connection, anchor, answer } = rig(80);
    connection.scrollSessionBy("s1", 100, 100);
    connection.sendInput("s1", new Uint8Array([97]));
    await answer();
    expect(anchor).not.toHaveBeenCalled();
  });
});
