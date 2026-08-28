import { describe, expect, it, vi } from "vitest";
import {
  YAS_CLASS_EVENT,
  YAS_CLASS_REQUEST,
  YAS_FAMILY_SURFACE,
  YAS_FAMILY_TERMINAL,
  YAS_SELECTION_ACTION_COPY,
  YAS_SURFACE_AXIS,
  YAS_SURFACE_CODEC_AV1_V1,
  YAS_SURFACE_CODEC_H264_V1,
  YAS_SURFACE_FRAME_END_OF_STREAM,
  YAS_SURFACE_FRAME_KEYFRAME,
  YAS_SURFACE_KEY,
  YAS_SURFACE_POINTER,
  YAS_SURFACE_PREEDIT,
  YAS_SURFACE_RESIZE_SCALE_120_EXTENSION,
  YAS_SURFACE_STATE,
  YAS_SURFACE_STATE_ACK,
  YAS_SURFACE_TEXT,
  YAS_SURFACE_TOUCH,
  YAS_SURFACE_UNWATCH,
  YAS_SURFACE_WATCH,
  YAS_STATUS_RESOURCE_EXHAUSTED,
  YAS_TERMINAL_COPY_RANGE,
  YAS_TERMINAL_STATE,
  YAS_TERMINAL_STATE_ACK,
  YAS_TERMINAL_UNWATCH,
  YAS_TERMINAL_WATCH,
} from "../yas/generated";
import { YasNativeWorkspaceConnection } from "../YasNativeWorkspaceConnection";
import * as YasSurfaceCanvas from "../YasSurfaceCanvas";
import { YasNativeProductFamilies } from "../yas/nativeProductFamilies";
import { encodeSurfaceCodecPayload } from "../yas/packed";
import { YasReceiveBudget } from "../yas/session";
import { YasProtocolError, YasResultError } from "../yas/wire";
import { CODEC_SUPPORT_AV1, CODEC_SUPPORT_H264 } from "../surfaceModel";

const flush = () => new Promise((resolve) => setTimeout(resolve, 0));

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: unknown) => void;
  const promise = new Promise<T>((yes, no) => {
    resolve = yes;
    reject = no;
  });
  return { promise, resolve, reject };
}

function surfaceTestView(codecVersion: number, firstSequence = 1n) {
  return {
    result: {
      firstSequence,
      maxInflightFrames: 3,
      codecVersion,
    },
    subscribe: vi.fn(() => vi.fn()),
    configure: vi.fn().mockResolvedValue(undefined),
    reset: vi.fn().mockResolvedValue(undefined),
    close: vi.fn().mockResolvedValue(undefined),
  };
}

function surfaceTestConnection(openView: ReturnType<typeof vi.fn>) {
  const surface = {
    limits: {
      maxViewDimension: 8192,
      maxViewPixels: 8192n * 8192n,
      maxFrameRate: 240,
    },
    openView,
  };
  const connection = Object.create(
    YasNativeWorkspaceConnection.prototype,
  ) as YasNativeWorkspaceConnection;
  Object.assign(connection as object, {
    disposed: false,
    pendingSurfaceViews: new Map(),
    surface,
    surfaceStreamingEnabled: true,
    displayFps: 120,
    surfaceMaxFps: 0,
    surfaceRecords: new Map([
      [
        1n,
        {
          logicalWidth32_32: 640n << 32n,
          logicalHeight32_32: 480n << 32n,
        },
      ],
    ]),
    surfaceMounts: new Map([
      [1n, new Map([["view", { target: null, maxFps: 0 }]])],
    ]),
    surfaceViews: new Map(),
    surfaceViewSizes: new Map(),
    views: new Map(),
    session: { ready: true },
    surfaceStore: { handleSurfaceEncoder: vi.fn() },
  });
  return {
    connection,
    lifecycle: connection as unknown as {
      refreshNativeSurfaceView(
        surfaceId: bigint,
        forceReset?: boolean,
      ): Promise<void>;
      pendingSurfaceViews: Map<
        bigint,
        { promise: Promise<void>; cancelled: boolean }
      >;
      surfaceViews: Map<bigint, unknown>;
    },
  };
}

describe("YasNativeWorkspaceConnection", () => {
  it("handles a rejected Font UNWATCH during invalidated disposal", async () => {
    const unwatch = vi
      .fn()
      .mockRejectedValue(new Error("session is not ready"));
    const disposeFont = vi.fn(() => {
      void unwatch().catch(() => undefined);
    });
    const dispose = vi.fn();
    const reset = vi.fn();
    const connection = Object.create(
      YasNativeWorkspaceConnection.prototype,
    ) as YasNativeWorkspaceConnection;
    Object.assign(connection as object, {
      disposed: false,
      removeCatalog: null,
      removeSelectionCatalog: null,
      removeSurfaceCatalog: null,
      removeSurfaceRemoteInput: null,
      removeSelectionGet: null,
      browserDrag: null,
      transport: { removeEventListener: vi.fn() },
      views: new Map(),
      pendingViews: new Map(),
      surfaceViews: new Map(),
      terminalClient: null,
      selectionClient: null,
      surface: null,
      channelFacade: null,
      extensionFacade: null,
      desktopMedia: null,
      fontProtocol: {
        dispose: disposeFont,
      },
      workspaceFs: { dispose },
      workspaceGit: { dispose },
      workspaceLsp: { dispose },
      workspaceKv: { dispose },
      native: { dispose },
      store: { destroy: dispose },
      surfaceStore: { destroy: dispose },
      audioPlayer: { destroy: dispose },
      desktopStore: { reset },
      mediaStore: { reset },
      listeners: new Set(),
      termCwdListeners: new Set(),
      readyListeners: new Set(),
    });

    connection.dispose();
    connection.dispose();
    await flush();

    expect(unwatch).toHaveBeenCalledOnce();
    expect(disposeFont).toHaveBeenCalledOnce();
  });

  it("deduplicates concurrent Terminal OPEN_VIEW requests per handle", async () => {
    const result = deferred<{
      result: {
        viewId: number;
        firstSequence: number;
        maxDecodedFrame: number;
        maxInflightFrames: number;
      };
      subscribe: (listener: (frame: never) => void) => () => void;
      close: () => Promise<void>;
    }>();
    const removeFrames = vi.fn();
    const view = {
      result: {
        viewId: 7,
        firstSequence: 1,
        maxDecodedFrame: 128,
        maxInflightFrames: 1,
      },
      subscribe: vi.fn(() => removeFrames),
      close: vi.fn().mockResolvedValue(undefined),
    };
    const cancelledResult = deferred<typeof view>();
    const cancelledView = {
      ...view,
      result: { ...view.result, viewId: 8 },
      subscribe: vi.fn(() => vi.fn()),
      close: vi.fn().mockResolvedValue(undefined),
    };
    const openView = vi
      .fn()
      .mockReturnValueOnce(result.promise)
      .mockReturnValueOnce(cancelledResult.promise);
    const connection = Object.create(
      YasNativeWorkspaceConnection.prototype,
    ) as YasNativeWorkspaceConnection;
    Object.assign(connection as object, {
      disposed: false,
      pendingViews: new Map(),
      desiredTerminalViews: new Set(),
      terminalViewAdmissionScheduled: false,
      terminalViewAdmissionRunning: false,
      terminalViewAdmissionWakePending: false,
      terminalViewAdmissionBlocker: null,
      terminalViewAdmissionEpoch: 0,
      views: new Map(),
      viewSizes: new Map(),
      records: new Map([
        [1n, { rows: 24, cols: 80 }],
        [2n, { rows: 24, cols: 80 }],
      ]),
      session: { ready: true },
      terminalClient: { openView, setFocus: vi.fn() },
      focusedSessionId: null,
      store: { handleUpdate: vi.fn() },
    });
    const lifecycle = connection as unknown as {
      openView(handle: bigint): Promise<void>;
      closeView(handle: bigint): Promise<void>;
      pendingViews: Map<bigint, { promise: Promise<void>; cancelled: boolean }>;
      views: Map<bigint, unknown>;
    };

    const first = lifecycle.openView(1n);
    const second = lifecycle.openView(1n);
    expect(openView).toHaveBeenCalledOnce();
    result.resolve(view);
    await Promise.all([first, second]);

    expect(lifecycle.views.size).toBe(1);
    expect(lifecycle.pendingViews.size).toBe(0);
    expect(view.close).not.toHaveBeenCalled();

    const late = lifecycle.openView(2n);
    await lifecycle.closeView(2n);
    cancelledResult.resolve(cancelledView);
    await late;
    expect(cancelledView.close).toHaveBeenCalledOnce();
    expect(lifecycle.views.has(2n)).toBe(false);
  });

  it("bounds over-budget terminal previews and retries after a view closes", async () => {
    const previewView = (viewId: number) => ({
      result: {
        viewId,
        firstSequence: 1,
        maxDecodedFrame: 128,
        maxInflightFrames: 1,
      },
      subscribe: vi.fn(() => vi.fn()),
      close: vi.fn().mockResolvedValue(undefined),
    });
    const firstPreview = previewView(11);
    const secondPreview = previewView(12);
    const openView = vi
      .fn()
      .mockRejectedValueOnce(
        new YasResultError(
          YAS_STATUS_RESOURCE_EXHAUSTED,
          new Uint8Array(),
          "aggregate YAS receive budget exhausted",
        ),
      )
      .mockResolvedValueOnce(firstPreview)
      .mockResolvedValueOnce(secondPreview);
    const activeView = {
      view: previewView(10),
      removeFrames: vi.fn(),
      grids: new Map(),
      pendingSequences: [],
      lastPresented: 0,
    };
    const connection = Object.create(
      YasNativeWorkspaceConnection.prototype,
    ) as YasNativeWorkspaceConnection;
    Object.assign(connection as object, {
      id: "remote",
      disposed: false,
      desiredTerminalViews: new Set(),
      terminalViewAdmissionScheduled: false,
      terminalViewAdmissionRunning: false,
      terminalViewAdmissionWakePending: false,
      terminalViewAdmissionBlocker: null,
      terminalViewAdmissionEpoch: 0,
      pendingViews: new Map(),
      views: new Map([[9n, activeView]]),
      viewSizes: new Map(),
      records: new Map([
        [1n, { rows: 24, cols: 80 }],
        [2n, { rows: 24, cols: 80 }],
        [9n, { rows: 24, cols: 80 }],
      ]),
      session: { ready: true },
      terminalClient: { openView, setFocus: vi.fn() },
      focusedSessionId: "remote:terminal:9",
      store: { handleUpdate: vi.fn() },
    });
    const lifecycle = connection as unknown as {
      subscribeTerminalView(handle: bigint): void;
      closeView(handle: bigint): Promise<void>;
      desiredTerminalViews: Set<bigint>;
      terminalViewAdmissionBlocker: bigint | null;
      pendingViews: Map<bigint, unknown>;
      views: Map<bigint, unknown>;
      focusedSessionId: string | null;
    };

    lifecycle.subscribeTerminalView(1n);
    lifecycle.subscribeTerminalView(2n);
    await vi.waitFor(() =>
      expect(lifecycle.terminalViewAdmissionBlocker).toBe(1n),
    );

    expect(openView).toHaveBeenCalledTimes(1);
    expect(lifecycle.desiredTerminalViews).toEqual(new Set([1n, 2n]));
    expect(lifecycle.pendingViews.size).toBe(0);
    expect(lifecycle.focusedSessionId).toBe("remote:terminal:9");

    await lifecycle.closeView(9n);
    await vi.waitFor(() => expect(openView).toHaveBeenCalledTimes(3));

    expect(
      openView.mock.calls.map(([request]) => request.terminalHandle),
    ).toEqual([1n, 1n, 2n]);
    expect(lifecycle.terminalViewAdmissionBlocker).toBeNull();
    expect(lifecycle.views.has(1n)).toBe(true);
    expect(lifecycle.views.has(2n)).toBe(true);
    expect(lifecycle.focusedSessionId).toBe("remote:terminal:9");
  });

  it("promotes a new focused view ahead of an older blocked preview", async () => {
    const terminalView = (viewId: number) => ({
      result: {
        viewId,
        firstSequence: 1,
        maxDecodedFrame: 128,
        maxInflightFrames: 1,
      },
      subscribe: vi.fn(() => vi.fn()),
      close: vi.fn().mockResolvedValue(undefined),
    });
    const exhausted = () =>
      new YasResultError(
        YAS_STATUS_RESOURCE_EXHAUSTED,
        new Uint8Array(),
        "aggregate YAS receive budget exhausted",
      );
    const promotedView = terminalView(12);
    const openView = vi
      .fn()
      .mockRejectedValueOnce(exhausted())
      .mockResolvedValueOnce(promotedView)
      .mockRejectedValueOnce(exhausted());
    let releaseCapacity = () => undefined;
    const previewView = terminalView(11);
    previewView.close.mockImplementation(async () => releaseCapacity());
    const activePreview = {
      view: previewView,
      removeFrames: vi.fn(),
      grids: new Map(),
      pendingSequences: [],
      lastPresented: 0,
    };
    const setFocus = vi.fn();
    const connection = Object.create(
      YasNativeWorkspaceConnection.prototype,
    ) as YasNativeWorkspaceConnection;
    Object.assign(connection as object, {
      id: "remote",
      disposed: false,
      desiredTerminalViews: new Set([1n]),
      terminalViewAdmissionScheduled: false,
      terminalViewAdmissionRunning: false,
      terminalViewAdmissionWakePending: false,
      terminalViewAdmissionBlocker: null,
      terminalViewAdmissionEpoch: 0,
      pendingViews: new Map(),
      views: new Map([[1n, activePreview]]),
      viewSizes: new Map(),
      records: new Map([
        [1n, { rows: 24, cols: 80 }],
        [2n, { rows: 24, cols: 80 }],
        [3n, { rows: 24, cols: 80 }],
      ]),
      sessions: new Map([
        ["remote:terminal:1", { ptyId: 1n, state: "active" }],
        ["remote:terminal:2", { ptyId: 2n, state: "active" }],
        ["remote:terminal:3", { ptyId: 3n, state: "active" }],
      ]),
      session: { ready: true },
      terminalClient: { openView, setFocus },
      focusedSessionId: "remote:terminal:1",
      store: {
        handleUpdate: vi.fn(),
        setDesiredSubscriptions: vi.fn(),
      },
    });
    const lifecycle = connection as unknown as {
      subscribeTerminalView(handle: bigint): void;
      onReceiveBudgetCapacity(): void;
      desiredTerminalViews: Set<bigint>;
      terminalViewAdmissionBlocker: bigint | null;
      views: Map<bigint, unknown>;
      focusedSessionId: string | null;
    };
    releaseCapacity = () => lifecycle.onReceiveBudgetCapacity();

    lifecycle.subscribeTerminalView(2n);
    await vi.waitFor(() =>
      expect(lifecycle.terminalViewAdmissionBlocker).toBe(2n),
    );
    expect(openView).toHaveBeenCalledTimes(1);
    expect(lifecycle.views.has(1n)).toBe(true);

    lifecycle.focusedSessionId = "remote:terminal:3";
    connection.setVisibleSessionIds([
      "remote:terminal:3",
      "remote:terminal:1",
      "remote:terminal:2",
    ]);
    await vi.waitFor(() =>
      expect(lifecycle.terminalViewAdmissionBlocker).toBe(1n),
    );

    expect(previewView.close).toHaveBeenCalledOnce();
    expect(
      openView.mock.calls.map(([request]) => request.terminalHandle),
    ).toEqual([2n, 3n, 1n]);
    expect(lifecycle.desiredTerminalViews).toEqual(new Set([3n, 1n, 2n]));

    expect(lifecycle.views.has(3n)).toBe(true);
    expect(lifecycle.views.has(1n)).toBe(false);
    expect(lifecycle.views.has(2n)).toBe(false);
    expect(setFocus).toHaveBeenCalledWith(promotedView.result.viewId, true);
    expect(lifecycle.focusedSessionId).toBe("remote:terminal:3");
    await flush();
    expect(openView).toHaveBeenCalledTimes(3);
  });

  it("reschedules a synchronous capacity release from priority eviction", async () => {
    const exhausted = () =>
      new YasResultError(
        YAS_STATUS_RESOURCE_EXHAUSTED,
        new Uint8Array(),
        "aggregate YAS receive budget exhausted",
      );
    const promotedView = {
      result: {
        viewId: 22,
        firstSequence: 1,
        maxDecodedFrame: 128,
        maxInflightFrames: 1,
      },
      subscribe: vi.fn(() => vi.fn()),
      close: vi.fn().mockResolvedValue(undefined),
    };
    const openView = vi
      .fn()
      .mockRejectedValueOnce(exhausted())
      .mockResolvedValueOnce(promotedView)
      .mockRejectedValueOnce(exhausted());
    let releaseCapacity = () => undefined;
    const closePreview = vi.fn(async () => releaseCapacity());
    const connection = Object.create(
      YasNativeWorkspaceConnection.prototype,
    ) as YasNativeWorkspaceConnection;
    Object.assign(connection as object, {
      id: "remote",
      disposed: false,
      desiredTerminalViews: new Set([2n, 1n]),
      terminalViewAdmissionScheduled: false,
      terminalViewAdmissionRunning: false,
      terminalViewAdmissionWakePending: false,
      terminalViewAdmissionBlocker: null,
      terminalViewAdmissionEpoch: 0,
      pendingViews: new Map(),
      views: new Map([
        [
          1n,
          {
            view: { close: closePreview },
            removeFrames: vi.fn(),
            grids: new Map(),
            pendingSequences: [],
            lastPresented: 0,
          },
        ],
      ]),
      viewSizes: new Map(),
      records: new Map([
        [1n, { rows: 24, cols: 80 }],
        [2n, { rows: 24, cols: 80 }],
      ]),
      session: { ready: true },
      terminalClient: { openView, setFocus: vi.fn() },
      focusedSessionId: "remote:terminal:2",
      store: { handleUpdate: vi.fn() },
    });
    const lifecycle = connection as unknown as {
      scheduleTerminalViewAdmissions(): void;
      onReceiveBudgetCapacity(): void;
      terminalViewAdmissionBlocker: bigint | null;
      terminalViewAdmissionWakePending: boolean;
      views: Map<bigint, unknown>;
    };
    releaseCapacity = () => lifecycle.onReceiveBudgetCapacity();

    lifecycle.scheduleTerminalViewAdmissions();
    await vi.waitFor(() =>
      expect(lifecycle.terminalViewAdmissionBlocker).toBe(1n),
    );

    expect(closePreview).toHaveBeenCalledOnce();
    expect(
      openView.mock.calls.map(([request]) => request.terminalHandle),
    ).toEqual([2n, 2n, 1n]);
    expect(lifecycle.views.has(2n)).toBe(true);
    expect(lifecycle.views.has(1n)).toBe(false);
    expect(lifecycle.terminalViewAdmissionWakePending).toBe(false);
    await flush();
    expect(openView).toHaveBeenCalledTimes(3);
  });

  it("retries when capacity releases before OPEN_VIEW reports exhaustion", async () => {
    const pendingOpen = deferred<never>();
    const view = {
      result: {
        viewId: 21,
        firstSequence: 1,
        maxDecodedFrame: 128,
        maxInflightFrames: 1,
      },
      subscribe: vi.fn(() => vi.fn()),
      close: vi.fn().mockResolvedValue(undefined),
    };
    const openView = vi
      .fn()
      .mockReturnValueOnce(pendingOpen.promise)
      .mockResolvedValueOnce(view);
    const connection = Object.create(
      YasNativeWorkspaceConnection.prototype,
    ) as YasNativeWorkspaceConnection;
    Object.assign(connection as object, {
      id: "remote",
      disposed: false,
      desiredTerminalViews: new Set(),
      terminalViewAdmissionScheduled: false,
      terminalViewAdmissionRunning: false,
      terminalViewAdmissionWakePending: false,
      terminalViewAdmissionBlocker: null,
      terminalViewAdmissionEpoch: 0,
      pendingViews: new Map(),
      views: new Map(),
      viewSizes: new Map(),
      records: new Map([[1n, { rows: 24, cols: 80 }]]),
      session: { ready: true },
      terminalClient: { openView, setFocus: vi.fn() },
      focusedSessionId: null,
      store: { handleUpdate: vi.fn() },
    });
    const lifecycle = connection as unknown as {
      subscribeTerminalView(handle: bigint): void;
      onReceiveBudgetCapacity(): void;
      terminalViewAdmissionBlocker: bigint | null;
      pendingViews: Map<bigint, unknown>;
      views: Map<bigint, unknown>;
    };

    lifecycle.subscribeTerminalView(1n);
    await vi.waitFor(() => expect(openView).toHaveBeenCalledOnce());
    lifecycle.onReceiveBudgetCapacity();
    pendingOpen.reject(
      new YasResultError(
        YAS_STATUS_RESOURCE_EXHAUSTED,
        new Uint8Array(),
        "aggregate YAS receive budget exhausted",
      ),
    );
    await vi.waitFor(() => expect(openView).toHaveBeenCalledTimes(2));

    expect(lifecycle.terminalViewAdmissionBlocker).toBeNull();
    expect(lifecycle.pendingViews.size).toBe(0);
    expect(lifecycle.views.has(1n)).toBe(true);
  });

  it("defers a late OPEN_VIEW failure across family reinitialization", async () => {
    const pendingOpen = deferred<never>();
    const view = {
      result: {
        viewId: 25,
        firstSequence: 1,
        maxDecodedFrame: 128,
        maxInflightFrames: 1,
      },
      subscribe: vi.fn(() => vi.fn()),
      close: vi.fn().mockResolvedValue(undefined),
    };
    const openView = vi
      .fn()
      .mockReturnValueOnce(pendingOpen.promise)
      .mockResolvedValueOnce(view);
    const connection = Object.create(
      YasNativeWorkspaceConnection.prototype,
    ) as YasNativeWorkspaceConnection;
    Object.assign(connection as object, {
      id: "remote",
      disposed: false,
      familyInitializationEpoch: 0,
      familyInitializationPending: false,
      familyInitializationError: null,
      desiredTerminalViews: new Set(),
      terminalViewAdmissionScheduled: false,
      terminalViewAdmissionRunning: false,
      terminalViewAdmissionWakePending: false,
      terminalViewAdmissionBlocker: null,
      terminalViewAdmissionEpoch: 0,
      pendingViews: new Map(),
      views: new Map(),
      viewSizes: new Map(),
      records: new Map([[1n, { rows: 24, cols: 80 }]]),
      session: { ready: true },
      terminalClient: { openView, setFocus: vi.fn() },
      focusedSessionId: null,
      store: { handleUpdate: vi.fn() },
    });
    const lifecycle = connection as unknown as {
      subscribeTerminalView(handle: bigint): void;
      scheduleTerminalViewAdmissions(): void;
      familyInitializationEpoch: number;
      familyInitializationPending: boolean;
      desiredTerminalViews: Set<bigint>;
      terminalViewAdmissionBlocker: bigint | null;
      views: Map<bigint, unknown>;
    };

    lifecycle.subscribeTerminalView(1n);
    await vi.waitFor(() => expect(openView).toHaveBeenCalledOnce());
    lifecycle.familyInitializationPending = true;
    lifecycle.familyInitializationEpoch++;
    lifecycle.familyInitializationPending = false;
    lifecycle.scheduleTerminalViewAdmissions();
    pendingOpen.reject(
      new YasProtocolError(
        "Terminal OPEN_VIEW completed after family invalidation",
      ),
    );
    await vi.waitFor(() => expect(openView).toHaveBeenCalledTimes(2));

    expect(lifecycle.terminalViewAdmissionBlocker).toBeNull();
    expect(lifecycle.desiredTerminalViews).toEqual(new Set([1n]));
    expect(lifecycle.views.has(1n)).toBe(true);
  });

  it("wakes blocked terminal admission when a Surface view releases capacity", async () => {
    const budget = new YasReceiveBudget(1n);
    const surfaceLease = budget.reserveExact(1n);
    const terminalView = {
      result: {
        viewId: 31,
        firstSequence: 1,
        maxDecodedFrame: 128,
        maxInflightFrames: 1,
      },
      subscribe: vi.fn(() => vi.fn()),
      close: vi.fn().mockResolvedValue(undefined),
    };
    const openView = vi.fn(async () => {
      budget.reserveExact(1n);
      return terminalView;
    });
    const closeSurface = vi.fn(async () => surfaceLease.release());
    const connection = Object.create(
      YasNativeWorkspaceConnection.prototype,
    ) as YasNativeWorkspaceConnection;
    Object.assign(connection as object, {
      id: "remote",
      disposed: false,
      desiredTerminalViews: new Set(),
      terminalViewAdmissionScheduled: false,
      terminalViewAdmissionRunning: false,
      terminalViewAdmissionWakePending: false,
      terminalViewAdmissionBlocker: null,
      terminalViewAdmissionEpoch: 0,
      pendingViews: new Map(),
      views: new Map(),
      viewSizes: new Map(),
      records: new Map([[1n, { rows: 24, cols: 80 }]]),
      pendingSurfaceViews: new Map(),
      surfaceViews: new Map([
        [9n, { removeFrames: vi.fn(), view: { close: closeSurface } }],
      ]),
      session: { ready: true },
      terminalClient: { openView, setFocus: vi.fn() },
      focusedSessionId: null,
      store: { handleUpdate: vi.fn() },
    });
    const lifecycle = connection as unknown as {
      subscribeTerminalView(handle: bigint): void;
      closeSurfaceView(surfaceId: bigint): Promise<void>;
      onReceiveBudgetCapacity(): void;
      terminalViewAdmissionBlocker: bigint | null;
      views: Map<bigint, unknown>;
    };
    const removeCapacityListener = budget.onCapacityAvailable(() =>
      lifecycle.onReceiveBudgetCapacity(),
    );

    try {
      lifecycle.subscribeTerminalView(1n);
      await vi.waitFor(() =>
        expect(lifecycle.terminalViewAdmissionBlocker).toBe(1n),
      );
      expect(openView).toHaveBeenCalledOnce();

      await lifecycle.closeSurfaceView(9n);
      await vi.waitFor(() => expect(openView).toHaveBeenCalledTimes(2));

      expect(closeSurface).toHaveBeenCalledOnce();
      expect(lifecycle.terminalViewAdmissionBlocker).toBeNull();
      expect(lifecycle.views.has(1n)).toBe(true);
    } finally {
      removeCapacityListener();
    }
  });

  it("admits a desired view when its catalogue record arrives, without being re-primed", async () => {
    // The visible set is known before the terminal is: a pane restored from a
    // layout names a session the catalogue has not delivered yet. Admission
    // skips it for want of a record, and the record arriving is what makes it
    // admissible — which is why that is where the retry lives now. It used to
    // live in callers re-priming admissions on every event, keystrokes
    // included.
    const view = {
      result: {
        viewId: 51,
        firstSequence: 1,
        maxDecodedFrame: 128,
        maxInflightFrames: 1,
      },
      subscribe: vi.fn(() => vi.fn()),
      close: vi.fn().mockResolvedValue(undefined),
    };
    const openView = vi.fn().mockResolvedValue(view);
    const connection = Object.create(
      YasNativeWorkspaceConnection.prototype,
    ) as YasNativeWorkspaceConnection;
    Object.assign(connection as object, {
      id: "remote",
      disposed: false,
      familyInitializationPending: false,
      familyInitializationError: null,
      familyInitializationEpoch: 0,
      desiredTerminalViews: new Set([1n]),
      terminalViewAdmissionScheduled: false,
      terminalViewAdmissionRunning: false,
      terminalViewAdmissionWakePending: false,
      terminalViewAdmissionBlocker: null,
      terminalViewAdmissionEpoch: 0,
      pendingViews: new Map(),
      views: new Map(),
      viewSizes: new Map(),
      records: new Map(),
      sessions: new Map(),
      termCwdListeners: new Set(),
      session: { ready: true },
      terminalClient: { openView, setFocus: vi.fn() },
      focusedSessionId: null,
      store: { handleUpdate: vi.fn() },
      snapshotListeners: new Set(),
      refreshSnapshot: vi.fn(),
      sessionId: (handle: bigint) => `s${handle}`,
      publicSession: () => ({ id: "s1", state: "active" }),
    });
    const lifecycle = connection as unknown as {
      scheduleTerminalViewAdmissions(): void;
      applyTerminalCatalog(records: readonly unknown[]): void;
      views: Map<bigint, unknown>;
    };

    lifecycle.scheduleTerminalViewAdmissions();
    await vi.waitFor(() => expect(openView).not.toHaveBeenCalled());

    lifecycle.applyTerminalCatalog([
      { handle: 1n, rows: 24, cols: 80, lifecycle: 0 },
    ]);
    await vi.waitFor(() => expect(openView).toHaveBeenCalledOnce());
    expect(lifecycle.views.has(1n)).toBe(true);
  });

  it("holds desired views while family initialization is in error", async () => {
    const view = {
      result: {
        viewId: 41,
        firstSequence: 1,
        maxDecodedFrame: 128,
        maxInflightFrames: 1,
      },
      subscribe: vi.fn(() => vi.fn()),
      close: vi.fn().mockResolvedValue(undefined),
    };
    const openView = vi.fn().mockResolvedValue(view);
    const connection = Object.create(
      YasNativeWorkspaceConnection.prototype,
    ) as YasNativeWorkspaceConnection;
    Object.assign(connection as object, {
      id: "remote",
      disposed: false,
      familyInitializationPending: false,
      familyInitializationError: "Terminal family bootstrap failed",
      desiredTerminalViews: new Set(),
      terminalViewAdmissionScheduled: false,
      terminalViewAdmissionRunning: false,
      terminalViewAdmissionWakePending: false,
      terminalViewAdmissionBlocker: null,
      terminalViewAdmissionEpoch: 0,
      pendingViews: new Map(),
      views: new Map(),
      viewSizes: new Map(),
      records: new Map([[1n, { rows: 24, cols: 80 }]]),
      session: { ready: true },
      terminalClient: { openView, setFocus: vi.fn() },
      focusedSessionId: null,
      store: { handleUpdate: vi.fn() },
    });
    const lifecycle = connection as unknown as {
      subscribeTerminalView(handle: bigint): void;
      scheduleTerminalViewAdmissions(): void;
      familyInitializationError: string | null;
      desiredTerminalViews: Set<bigint>;
      views: Map<bigint, unknown>;
    };

    lifecycle.subscribeTerminalView(1n);
    await flush();
    expect(openView).not.toHaveBeenCalled();
    expect(lifecycle.desiredTerminalViews).toEqual(new Set([1n]));

    lifecycle.familyInitializationError = null;
    lifecycle.scheduleTerminalViewAdmissions();
    await vi.waitFor(() => expect(openView).toHaveBeenCalledOnce());
    expect(lifecycle.views.has(1n)).toBe(true);
  });

  it("deduplicates concurrent Surface OPEN_VIEW requests per handle", async () => {
    const result = deferred<{
      result: {
        firstSequence: bigint;
        maxInflightFrames: number;
        codecVersion: number;
      };
      subscribe: (listener: (frame: never) => void) => () => void;
      configure: (options: unknown) => Promise<void>;
      reset: () => Promise<void>;
      close: () => Promise<void>;
    }>();
    const view = surfaceTestView(YAS_SURFACE_CODEC_H264_V1);
    const cancelledResult = deferred<typeof view>();
    const cancelledView = {
      ...view,
      result: { ...view.result, firstSequence: 2n },
      subscribe: vi.fn(() => vi.fn()),
      close: vi.fn().mockResolvedValue(undefined),
    };
    const openView = vi
      .fn()
      .mockReturnValueOnce(result.promise)
      .mockReturnValueOnce(cancelledResult.promise);
    const { connection, lifecycle } = surfaceTestConnection(openView);
    (
      connection as unknown as {
        surfaceRecords: Map<bigint, unknown>;
        surfaceMounts: Map<bigint, Map<string, unknown>>;
      }
    ).surfaceRecords.set(2n, {
      logicalWidth32_32: 640n << 32n,
      logicalHeight32_32: 480n << 32n,
    });
    (
      connection as unknown as {
        surfaceMounts: Map<bigint, Map<string, unknown>>;
      }
    ).surfaceMounts.set(2n, new Map([["view", { target: null, maxFps: 0 }]]));

    const first = lifecycle.refreshNativeSurfaceView(1n);
    const second = lifecycle.refreshNativeSurfaceView(1n);
    expect(openView).toHaveBeenCalledOnce();
    expect(openView).toHaveBeenLastCalledWith(
      expect.objectContaining({ decoderCapacity: 16, maxFps: 120 }),
    );
    result.resolve(view);
    await Promise.all([first, second]);

    expect(openView).toHaveBeenCalledOnce();
    // The second waiter re-evaluates the now-open view, but identical
    // parameters must not reset its encoder.
    expect(view.configure).not.toHaveBeenCalled();
    expect(lifecycle.surfaceViews.size).toBe(1);
    expect(lifecycle.pendingSurfaceViews.size).toBe(0);
    expect(view.close).not.toHaveBeenCalled();

    const late = lifecycle.refreshNativeSurfaceView(2n);
    connection.sendSurfaceUnsubscribe(2n, "view");
    cancelledResult.resolve(cancelledView);
    await late;
    expect(cancelledView.close).toHaveBeenCalledOnce();
    expect(lifecycle.surfaceViews.has(2n)).toBe(false);
  });

  it("closes a Surface OPEN_VIEW that resolves after streaming is disabled", async () => {
    const result = deferred<ReturnType<typeof surfaceTestView>>();
    const openView = vi.fn().mockReturnValue(result.promise);
    const { connection, lifecycle } = surfaceTestConnection(openView);

    const opening = lifecycle.refreshNativeSurfaceView(1n);
    connection.setSurfaceStreamingEnabled(false);
    const view = surfaceTestView(YAS_SURFACE_CODEC_H264_V1);
    result.resolve(view);
    await opening;

    expect(view.close).toHaveBeenCalledOnce();
    expect(lifecycle.surfaceViews.has(1n)).toBe(false);
    expect(lifecycle.pendingSurfaceViews.size).toBe(0);
  });

  it("closes and reopens a pending Surface view after codec policy changes", async () => {
    const oldResult = deferred<ReturnType<typeof surfaceTestView>>();
    const newResult = deferred<ReturnType<typeof surfaceTestView>>();
    const openView = vi
      .fn()
      .mockReturnValueOnce(oldResult.promise)
      .mockReturnValueOnce(newResult.promise);
    const codecSupport = vi
      .spyOn(YasSurfaceCanvas, "getCodecSupport")
      .mockReturnValue(CODEC_SUPPORT_H264);
    try {
      const { connection, lifecycle } = surfaceTestConnection(openView);
      const first = lifecycle.refreshNativeSurfaceView(1n);
      expect(openView).toHaveBeenCalledWith(
        expect.objectContaining({
          decoderCapacity: 16,
          codecVersions: expect.arrayContaining([YAS_SURFACE_CODEC_H264_V1]),
        }),
      );

      codecSupport.mockReturnValue(CODEC_SUPPORT_AV1);
      connection.refreshCodecSupport();
      const oldView = surfaceTestView(YAS_SURFACE_CODEC_H264_V1);
      oldResult.resolve(oldView);
      await first;
      await vi.waitFor(() => expect(openView).toHaveBeenCalledTimes(2));

      expect(oldView.close).toHaveBeenCalledOnce();
      expect(lifecycle.surfaceViews.has(1n)).toBe(false);
      expect(openView).toHaveBeenLastCalledWith(
        expect.objectContaining({
          decoderCapacity: 16,
          codecVersions: expect.arrayContaining([YAS_SURFACE_CODEC_AV1_V1]),
        }),
      );
      expect(openView.mock.calls.at(-1)?.[0].codecVersions).not.toContain(
        YAS_SURFACE_CODEC_H264_V1,
      );

      const newView = surfaceTestView(YAS_SURFACE_CODEC_AV1_V1, 2n);
      newResult.resolve(newView);
      await vi.waitFor(() => expect(lifecycle.surfaceViews.has(1n)).toBe(true));
      expect(newView.close).not.toHaveBeenCalled();
    } finally {
      codecSupport.mockRestore();
    }
  });

  it("does not reset a closed Surface view after CONFIGURE settles", async () => {
    const configured = deferred<void>();
    const openView = vi.fn();
    const { connection, lifecycle } = surfaceTestConnection(openView);
    const view = surfaceTestView(YAS_SURFACE_CODEC_H264_V1);
    view.configure.mockReturnValue(configured.promise);
    lifecycle.surfaceViews.set(1n, {
      view,
      removeFrames: vi.fn(),
      width: 640,
      height: 480,
      maxFps: 60,
      lastReceived: 0n,
      lastPresented: 0n,
      decoderQueueDepth: 0,
    });

    const updating = lifecycle.refreshNativeSurfaceView(1n, true);
    connection.setSurfaceStreamingEnabled(false);
    configured.reject(new Error("view is closed"));

    await expect(updating).resolves.toBeUndefined();
    expect(view.close).toHaveBeenCalledOnce();
    expect(view.reset).not.toHaveBeenCalled();
    expect(lifecycle.surfaceViews.has(1n)).toBe(false);
  });

  it("strips Surface codec metadata and does not decode EOS", () => {
    const handleSurfaceFrame = vi.fn();
    const connection = Object.create(
      YasNativeWorkspaceConnection.prototype,
    ) as YasNativeWorkspaceConnection;
    Object.assign(connection as object, {
      surfaceStore: { handleSurfaceFrame },
    });
    const bitstream = new Uint8Array([0, 0, 0, 1, 0x65, 0x88]);
    const view = {
      result: {
        viewId: 7,
        firstSequence: 1n,
        maxInflightFrames: 1,
        codecVersion: YAS_SURFACE_CODEC_H264_V1,
      },
    };
    const state = {
      view,
      width: 640,
      height: 480,
      lastReceived: 0n,
      lastPresented: 0n,
      decoderQueueDepth: 0,
    };
    const payload = encodeSurfaceCodecPayload(YAS_SURFACE_CODEC_H264_V1, {
      damage: [{ x: 1, y: 2, width: 3, height: 4 }],
      bitstream,
    });

    const lifecycle = connection as unknown as {
      acceptSurfaceFrame(
        surfaceId: bigint,
        state: typeof state,
        frame: {
          viewId: number;
          sequence: bigint;
          baseSequence: bigint;
          captureNs: bigint;
          presentationNs: bigint;
          flags: number;
          codecVersion: number;
          fragmentIndex: number;
          fragmentCount: number;
          completeLength: number;
          payload: Uint8Array;
        },
      ): void;
    };
    lifecycle.acceptSurfaceFrame(1n, state, {
      viewId: 7,
      sequence: 1n,
      baseSequence: 0n,
      captureNs: 1_001_000n,
      presentationNs: 0n,
      flags: YAS_SURFACE_FRAME_KEYFRAME,
      codecVersion: YAS_SURFACE_CODEC_H264_V1,
      fragmentIndex: 0,
      fragmentCount: 1,
      completeLength: payload.length,
      payload,
    });

    expect(handleSurfaceFrame).toHaveBeenCalledOnce();
    expect(handleSurfaceFrame.mock.calls[0]?.[5]).toEqual(bitstream);

    handleSurfaceFrame.mockClear();
    lifecycle.acceptSurfaceFrame(1n, state, {
      viewId: 7,
      sequence: 2n,
      baseSequence: 0n,
      captureNs: 2_001_000n,
      presentationNs: 0n,
      flags: YAS_SURFACE_FRAME_KEYFRAME | YAS_SURFACE_FRAME_END_OF_STREAM,
      codecVersion: YAS_SURFACE_CODEC_H264_V1,
      fragmentIndex: 0,
      fragmentCount: 1,
      completeLength: 4,
      payload: new Uint8Array(4),
    });
    expect(handleSurfaceFrame).not.toHaveBeenCalled();
    expect(state.lastReceived).toBe(2n);
  });

  it("sends logical Surface dimensions with the measured 2x scale", () => {
    const resize = vi.fn();
    const connection = Object.create(
      YasNativeWorkspaceConnection.prototype,
    ) as YasNativeWorkspaceConnection;
    Object.assign(connection as object, {
      surface: { resize },
      surfaceMounts: new Map(),
      surfaceViewSizes: new Map(),
      session: { ready: true },
    });

    expect(connection.offerSurfaceViewSize(1n, "pane", 1600, 1200, 240)).toBe(
      true,
    );
    expect(resize).toHaveBeenCalledWith(
      1n,
      expect.any(Uint8Array),
      800n << 32n,
      600n << 32n,
      [
        {
          tag: YAS_SURFACE_RESIZE_SCALE_120_EXTENSION,
          required: true,
          value: new Uint8Array([240, 0]),
        },
      ],
    );
  });

  it("maps a 2x Surface catalogue record into physical pointer space", () => {
    const handleSurfaceCreated = vi.fn();
    const handleSurfaceResized = vi.fn();
    const connection = Object.create(
      YasNativeWorkspaceConnection.prototype,
    ) as YasNativeWorkspaceConnection;
    Object.assign(connection as object, {
      surfaceRecords: new Map(),
      surfaceStore: {
        handleSurfaceCreated,
        handleSurfaceResized,
        handleSurfaceDestroyed: vi.fn(),
        handleSurfaceTitle: vi.fn(),
        handleSurfaceAppId: vi.fn(),
      },
      surfaceViews: new Map(),
      emit: vi.fn(),
    });
    const lifecycle = connection as unknown as {
      applySurfaceCatalog(records: readonly unknown[]): void;
    };
    const record = {
      surfaceHandle: 1n,
      revision: 1n,
      parentHandle: 0n,
      appHandle: 0n,
      lifecycle: 0,
      bufferScale: 2,
      logicalWidth32_32: 800n << 32n,
      logicalHeight32_32: 600n << 32n,
      applicationId: "app",
      title: "title",
      extensions: [],
    };

    lifecycle.applySurfaceCatalog([record]);
    expect(handleSurfaceCreated).toHaveBeenCalledWith(
      1n,
      0n,
      1600,
      1200,
      "title",
      "app",
    );
    expect(handleSurfaceResized).toHaveBeenCalledWith(
      1n,
      1600,
      1200,
      800,
      600,
    );

    handleSurfaceResized.mockClear();
    lifecycle.applySurfaceCatalog([{ ...record, bufferScale: 1 }]);
    expect(handleSurfaceResized).toHaveBeenCalledWith(
      1n,
      800,
      600,
      800,
      600,
    );
  });

  it("opens an unscaled HiDPI Surface view at physical size and display rate", async () => {
    const view = surfaceTestView(YAS_SURFACE_CODEC_AV1_V1);
    const openView = vi.fn().mockResolvedValue(view);
    const { connection, lifecycle } = surfaceTestConnection(openView);
    (
      connection as unknown as {
        surfaceViewSizes: Map<
          bigint,
          Map<string, { width: number; height: number; scale120: number }>
        >;
      }
    ).surfaceViewSizes.set(
      1n,
      new Map([["view", { width: 1600, height: 1200, scale120: 240 }]]),
    );

    await lifecycle.refreshNativeSurfaceView(1n);

    expect(openView).toHaveBeenCalledWith(
      expect.objectContaining({ width: 1600, height: 1200, maxFps: 120 }),
    );
  });

  it("reconfigures uncapped Surface views when display refresh changes", async () => {
    const view = surfaceTestView(YAS_SURFACE_CODEC_AV1_V1);
    const openView = vi.fn().mockResolvedValue(view);
    const { connection, lifecycle } = surfaceTestConnection(openView);
    await lifecycle.refreshNativeSurfaceView(1n);

    (
      connection as unknown as {
        configureDisplayRate(fps: number): void;
      }
    ).configureDisplayRate(144);

    await vi.waitFor(() =>
      expect(view.configure).toHaveBeenCalledWith(
        expect.objectContaining({ maxFps: 144 }),
      ),
    );
  });

  it("reopens exactly once when codec policy changes during CONFIGURE", async () => {
    const configured = deferred<void>();
    const reopened = deferred<ReturnType<typeof surfaceTestView>>();
    const openView = vi.fn().mockReturnValue(reopened.promise);
    const codecSupport = vi
      .spyOn(YasSurfaceCanvas, "getCodecSupport")
      .mockReturnValue(CODEC_SUPPORT_H264);
    try {
      const { connection, lifecycle } = surfaceTestConnection(openView);
      const oldView = surfaceTestView(YAS_SURFACE_CODEC_H264_V1);
      oldView.configure.mockReturnValue(configured.promise);
      lifecycle.surfaceViews.set(1n, {
        view: oldView,
        removeFrames: vi.fn(),
        width: 640,
        height: 480,
        maxFps: 60,
        lastReceived: 0n,
        lastPresented: 0n,
        decoderQueueDepth: 0,
      });

      const updating = lifecycle.refreshNativeSurfaceView(1n, true);
      codecSupport.mockReturnValue(CODEC_SUPPORT_AV1);
      connection.refreshCodecSupport();
      configured.resolve();
      await updating;
      await vi.waitFor(() => expect(openView).toHaveBeenCalledOnce());

      expect(oldView.close).toHaveBeenCalledOnce();
      expect(oldView.reset).not.toHaveBeenCalled();
      expect(openView).toHaveBeenCalledWith(
        expect.objectContaining({
          decoderCapacity: 16,
          codecVersions: expect.arrayContaining([YAS_SURFACE_CODEC_AV1_V1]),
        }),
      );
      const newView = surfaceTestView(YAS_SURFACE_CODEC_AV1_V1, 2n);
      reopened.resolve(newView);
      await vi.waitFor(() => expect(lifecycle.surfaceViews.has(1n)).toBe(true));
      expect(openView).toHaveBeenCalledOnce();
    } finally {
      codecSupport.mockRestore();
    }
  });

  it("publishes malformed family bootstrap as a settled error", async () => {
    const failure = new Error("corrupt family descriptor");
    const reportError = vi.fn();
    const handleStatusChange = vi.fn();
    const connection = Object.create(
      YasNativeWorkspaceConnection.prototype,
    ) as YasNativeWorkspaceConnection;
    Object.assign(connection as object, {
      disposed: false,
      familyInitializationEpoch: 0,
      familyInitializationPending: false,
      familyInitializationError: null,
      familyReconfigurationNeeded: false,
      familyInitializationQueued: false,
      familyInitializationRunning: false,
      familyGenerationBumpPending: false,
      generation: 0,
      session: {
        ready: true,
        hello: null,
        family: vi.fn(() => {
          throw failure;
        }),
      },
      transport: { status: "connected", lastError: null },
      removeCatalog: null,
      snapshot: { id: "remote", status: "connected", ready: true },
      listeners: new Set(),
      store: { handleStatusChange },
    });
    const lifecycle = connection as unknown as {
      onSessionReady(): void;
      familyInitializationPending: boolean;
      familyInitializationQueued: boolean;
      familyInitializationRunning: boolean;
      snapshot: { status: string; ready: boolean; error: string | null };
    };
    vi.stubGlobal("reportError", reportError);

    try {
      lifecycle.onSessionReady();
      await flush();

      expect(lifecycle.snapshot).toMatchObject({
        status: "error",
        ready: false,
        error: failure.message,
      });
      expect(lifecycle.familyInitializationPending).toBe(false);
      expect(lifecycle.familyInitializationQueued).toBe(false);
      expect(lifecycle.familyInitializationRunning).toBe(false);
      expect(handleStatusChange).toHaveBeenCalledWith("error");
      expect(reportError).toHaveBeenCalledOnce();
      expect(reportError).toHaveBeenCalledWith(failure);
    } finally {
      vi.unstubAllGlobals();
    }
  });

  it("serializes catalogue bootstrap across reconnect and reconfiguration", async () => {
    const currentInitializationError = new Error("terminal WATCH failed");
    let resolveFirstSurfaceWatch!: () => void;
    const firstSurfaceWatch = new Promise<void>((resolve) => {
      resolveFirstSurfaceWatch = resolve;
    });
    const watch = vi.fn().mockResolvedValue(undefined);
    const removeCatalogs = [vi.fn(), vi.fn(), vi.fn(), vi.fn(), vi.fn()];
    const subscribe = vi.fn();
    for (const remove of removeCatalogs) subscribe.mockReturnValueOnce(remove);
    let surfaceWatchActive = false;
    const surfaceWireWatches = vi.fn();
    const surfaceUnwatch = vi.fn(async () => {
      surfaceWatchActive = false;
    });
    const surfaceWatch = vi.fn(() => {
      if (surfaceWatchActive) return Promise.resolve();
      surfaceWatchActive = true;
      surfaceWireWatches();
      if (surfaceWireWatches.mock.calls.length === 1) return firstSurfaceWatch;
      return Promise.resolve();
    });
    const surfaceDispose = vi.fn();
    const surface = {
      catalog: {
        subscribe: vi.fn(() => vi.fn()),
        watch: surfaceWatch,
        unwatch: surfaceUnwatch,
      },
      onRemoteInput: vi.fn(() => vi.fn()),
      dispose: surfaceDispose,
    };
    let terminalCatalogueAvailable = true;
    const session = {
      ready: true,
      hello: null,
      families: new Map([[YAS_FAMILY_TERMINAL, {}]]),
      family: vi.fn(() => ({})),
      operationAdvertised: vi.fn((family: number) => {
        if (family === YAS_FAMILY_TERMINAL) return terminalCatalogueAvailable;
        return family === YAS_FAMILY_SURFACE;
      }),
    };
    const transport = { status: "connected", lastError: null };
    const handleStatusChange = vi.fn();
    const closeViewsLocal = vi.fn();
    const scheduleTerminalViewAdmissions = vi.fn();
    const reconcileTerminalViewAdmissionPriority = vi.fn();
    const terminalDispose = vi.fn();
    const setNativeController = vi.fn();
    const reset = vi.fn();
    const reportError = vi.fn();
    const connection = Object.create(
      YasNativeWorkspaceConnection.prototype,
    ) as YasNativeWorkspaceConnection;
    Object.assign(connection as object, {
      disposed: false,
      familyInitializationEpoch: 0,
      familyInitializationPending: false,
      familyInitializationError: null,
      familyReconfigurationNeeded: false,
      generation: 0,
      id: "remote",
      session,
      transport,
      terminalClient: {
        catalog: { subscribe, watch },
        dispose: terminalDispose,
      },
      removeCatalog: null,
      surface,
      removeSurfaceCatalog: null,
      removeSurfaceRemoteInput: null,
      surfaceViews: new Map(),
      surfaceRecords: new Map(),
      surfaceStore: { handleSurfaceDestroyed: vi.fn() },
      selectionClient: null,
      fontProtocol: null,
      desktopMedia: null,
      native: {
        supports: vi.fn(() => false),
        channel: null,
        extension: null,
        fs: null,
        git: null,
        kv: null,
        lsp: null,
      },
      desktopStore: { setNativeController, reset },
      mediaStore: {
        setNativeController,
        reset,
        publishNativeLease: vi.fn(),
        publishNativeCameraTrack: vi.fn(),
      },
      mprisStore: { setNativeController, reset },
      audioPlayer: { reset },
      store: { handleStatusChange, isReady: () => true },
      closeViewsLocal,
      scheduleTerminalViewAdmissions,
      reconcileTerminalViewAdmissionPriority,
      records: new Map(),
      sessions: new Map(),
      focusedSessionId: null,
      snapshot: { id: "remote", ready: false },
      listeners: new Set(),
      readyListeners: new Set(),
    });
    const lifecycle = connection as unknown as {
      onSessionReady(): void;
      onSessionInvalidation(invalidation: {
        family?: number;
        error: Error;
      }): void;
      onSessionCatalogChange(): void;
      refreshSnapshot(): void;
      familyInitializationPending: boolean;
      familyInitializationError: string | null;
      snapshot: {
        status: string;
        ready: boolean;
        error: string | null;
        generation: number;
        supportsCopyRange: boolean;
      };
    };
    vi.stubGlobal("reportError", reportError);

    try {
      session.ready = false;
      lifecycle.refreshSnapshot();
      expect(lifecycle.snapshot).toMatchObject({
        status: "authenticating",
        ready: false,
      });
      session.ready = true;

      lifecycle.onSessionReady();
      expect(watch).toHaveBeenCalledOnce();
      await Promise.resolve();
      expect(surfaceWireWatches).toHaveBeenCalledOnce();
      expect(lifecycle.snapshot).toMatchObject({
        status: "authenticating",
        ready: false,
        error: null,
      });

      lifecycle.onSessionCatalogChange();
      await Promise.resolve();
      expect(watch).toHaveBeenCalledOnce();
      expect(surfaceWatch).toHaveBeenCalledOnce();

      session.ready = false;
      transport.status = "disconnected";
      lifecycle.onSessionInvalidation({
        error: new Error("link disconnected"),
      });
      resolveFirstSurfaceWatch();
      await flush();
      expect(surfaceUnwatch).toHaveBeenCalledOnce();

      session.ready = true;
      transport.status = "connected";
      lifecycle.onSessionReady();
      await flush();

      expect(watch).toHaveBeenCalledTimes(2);
      expect(surfaceWireWatches).toHaveBeenCalledTimes(2);
      expect(removeCatalogs[0]).toHaveBeenCalledOnce();
      expect(handleStatusChange).toHaveBeenCalledWith("connected");
      expect(reconcileTerminalViewAdmissionPriority).toHaveBeenCalledOnce();
      expect(scheduleTerminalViewAdmissions).toHaveBeenCalledOnce();
      expect(lifecycle.familyInitializationPending).toBe(false);
      expect(lifecycle.familyInitializationError).toBeNull();
      expect(reportError).not.toHaveBeenCalled();

      const generationAfterHello = lifecycle.snapshot.generation;
      lifecycle.onSessionCatalogChange();
      await flush();
      expect(watch).toHaveBeenCalledTimes(3);
      expect(surfaceWatch).toHaveBeenCalledTimes(3);
      expect(surfaceWireWatches).toHaveBeenCalledTimes(2);
      expect(lifecycle.snapshot.generation).toBe(generationAfterHello);

      const closesBeforeFamilyUpdate = closeViewsLocal.mock.calls.length;
      lifecycle.onSessionInvalidation({
        family: YAS_FAMILY_TERMINAL,
        error: new Error("terminal descriptor changed"),
      });
      expect(closeViewsLocal).toHaveBeenCalledTimes(closesBeforeFamilyUpdate);
      lifecycle.onSessionCatalogChange();
      await flush();
      expect(watch).toHaveBeenCalledTimes(4);
      expect(surfaceWatch).toHaveBeenCalledTimes(4);
      expect(surfaceWireWatches).toHaveBeenCalledTimes(2);
      expect(surfaceDispose).not.toHaveBeenCalled();
      expect(lifecycle.snapshot.generation).toBe(generationAfterHello + 1);

      watch.mockRejectedValueOnce(currentInitializationError);
      lifecycle.onSessionCatalogChange();
      await flush();
      expect(lifecycle.snapshot).toMatchObject({
        status: "error",
        ready: false,
        error: currentInitializationError.message,
      });
      expect(reportError).toHaveBeenCalledWith(currentInitializationError);

      terminalCatalogueAvailable = false;
      lifecycle.onSessionCatalogChange();
      await flush();
      expect(terminalDispose).toHaveBeenCalledOnce();
      expect(watch).toHaveBeenCalledTimes(5);
      expect(surfaceWireWatches).toHaveBeenCalledTimes(2);
      expect(lifecycle.snapshot).toMatchObject({
        status: "connected",
        ready: true,
        supportsCopyRange: false,
        error: null,
      });
    } finally {
      vi.unstubAllGlobals();
    }
  });

  it("does not advertise or send narrowed presentation operations", () => {
    const advertised = new Set<string>();
    const key = (
      family: number,
      frameClass: number,
      kind: number,
      serverSends = false,
    ) => `${family}/${frameClass}/${kind}/${serverSends}`;
    const enable = (
      family: number,
      frameClass: number,
      kind: number,
      serverSends = false,
    ) => advertised.add(key(family, frameClass, kind, serverSends));
    for (const [family, watchKind, unwatchKind, stateKind, ackKind] of [
      [
        YAS_FAMILY_TERMINAL,
        YAS_TERMINAL_WATCH,
        YAS_TERMINAL_UNWATCH,
        YAS_TERMINAL_STATE,
        YAS_TERMINAL_STATE_ACK,
      ],
      [
        YAS_FAMILY_SURFACE,
        YAS_SURFACE_WATCH,
        YAS_SURFACE_UNWATCH,
        YAS_SURFACE_STATE,
        YAS_SURFACE_STATE_ACK,
      ],
    ] as const) {
      enable(family, YAS_CLASS_REQUEST, watchKind);
      enable(family, YAS_CLASS_REQUEST, unwatchKind);
      enable(family, YAS_CLASS_EVENT, stateKind, true);
      enable(family, YAS_CLASS_EVENT, ackKind);
    }
    const text = vi.fn();
    const preedit = vi.fn();
    const touch = vi.fn();
    const keyEvent = vi.fn();
    const pointer = vi.fn();
    const axis = vi.fn();
    const view = { result: { maxInflightFrames: 4 } };
    const connection = Object.create(
      YasNativeWorkspaceConnection.prototype,
    ) as YasNativeWorkspaceConnection;
    Object.assign(connection as object, {
      id: "remote",
      disposed: false,
      familyInitializationPending: false,
      familyInitializationError: null,
      generation: 1,
      session: {
        ready: true,
        hello: null,
        family: vi.fn(() => ({})),
        operationAdvertised: vi.fn(
          (
            family: number,
            frameClass: number,
            kind: number,
            serverSends = false,
          ) => advertised.has(key(family, frameClass, kind, serverSends)),
        ),
      },
      transport: { status: "connected", lastError: null },
      surface: { text, preedit, touch, key: keyEvent, pointer, axis },
      surfaceViews: new Map([
        [
          1n,
          {
            view,
            lastPresented: 0n,
            decoderQueueDepth: 0,
          },
        ],
      ]),
      surfaceTouchUsers: 1,
      pressedSurfaceKeys: new Set(),
      snapshot: { id: "remote", ready: false },
      native: {
        supports: vi.fn(() => false),
        channel: null,
        extension: null,
        fs: null,
        git: null,
        kv: null,
        lsp: null,
      },
      desktopMedia: null,
      sessions: new Map(),
      focusedSessionId: null,
      store: { isReady: () => true },
      listeners: new Set(),
      readyListeners: new Set(),
    });
    const refresh = () =>
      (
        connection as unknown as {
          refreshSnapshot(): void;
        }
      ).refreshSnapshot();
    const snapshot = () =>
      (
        connection as unknown as {
          snapshot: {
            supportsCopyRange: boolean;
            supportsSurfaceTouch: boolean;
            supportsSurfaceTextInput: boolean;
          };
        }
      ).snapshot;

    refresh();
    expect(connection.supportsCopyRange()).toBe(false);
    expect(connection.supportsSurfaceTouch).toBe(false);
    expect(connection.supportsSurfaceTextInput).toBe(false);
    expect(snapshot()).toMatchObject({
      supportsCopyRange: false,
      supportsSurfaceTouch: false,
      supportsSurfaceTextInput: false,
    });
    connection.sendSurfaceText(1n, "text");
    connection.sendSurfacePreedit(1n, "preedit", 7);
    connection.sendSurfaceTouch(1n, 0);
    connection.sendSurfaceInput(1n, 30, true);
    connection.sendSurfacePointer(1n, 0, 0, 1, 1);
    connection.sendSurfaceAxis(1n, 0, 100);
    expect(text).not.toHaveBeenCalled();
    expect(preedit).not.toHaveBeenCalled();
    expect(touch).not.toHaveBeenCalled();
    expect(keyEvent).not.toHaveBeenCalled();
    expect(pointer).not.toHaveBeenCalled();
    expect(axis).not.toHaveBeenCalled();

    enable(YAS_FAMILY_TERMINAL, YAS_CLASS_REQUEST, YAS_TERMINAL_COPY_RANGE);
    enable(YAS_FAMILY_SURFACE, YAS_CLASS_EVENT, YAS_SURFACE_TEXT);
    refresh();
    expect(connection.supportsCopyRange()).toBe(true);
    expect(connection.supportsSurfaceTextInput).toBe(false);
    connection.sendSurfaceText(1n, "text");
    connection.sendSurfacePreedit(1n, "preedit", 7);
    expect(text).toHaveBeenCalledOnce();
    expect(preedit).not.toHaveBeenCalled();

    enable(YAS_FAMILY_SURFACE, YAS_CLASS_EVENT, YAS_SURFACE_PREEDIT);
    enable(YAS_FAMILY_SURFACE, YAS_CLASS_EVENT, YAS_SURFACE_TOUCH);
    enable(YAS_FAMILY_SURFACE, YAS_CLASS_EVENT, YAS_SURFACE_KEY);
    enable(YAS_FAMILY_SURFACE, YAS_CLASS_EVENT, YAS_SURFACE_POINTER);
    enable(YAS_FAMILY_SURFACE, YAS_CLASS_EVENT, YAS_SURFACE_AXIS);
    refresh();
    expect(connection.supportsSurfaceTouch).toBe(true);
    expect(connection.supportsSurfaceTextInput).toBe(true);
    expect(snapshot()).toMatchObject({
      supportsCopyRange: true,
      supportsSurfaceTouch: true,
      supportsSurfaceTextInput: true,
    });
    connection.sendSurfacePreedit(1n, "preedit", 7);
    connection.sendSurfaceTouch(1n, 0);
    connection.sendSurfaceInput(1n, 30, true);
    connection.sendSurfacePointer(1n, 0, 0, 1, 1);
    connection.sendSurfaceAxis(1n, 0, 100);
    expect(preedit).toHaveBeenCalledOnce();
    expect(touch).toHaveBeenCalledOnce();
    expect(keyEvent).toHaveBeenCalledOnce();
    expect(pointer).toHaveBeenCalledOnce();
    expect(axis).toHaveBeenCalledOnce();
  });

  it("publishes the full native HELLO boot ID as the restart discriminator", () => {
    const connection = Object.create(
      YasNativeWorkspaceConnection.prototype,
    ) as YasNativeWorkspaceConnection;
    Object.assign(connection as object, {
      id: "remote",
      session: {
        ready: true,
        hello: {
          bootId: new Uint8Array([
            0xff, 0xee, 0xdd, 0xcc, 0xbb, 0xaa, 0x99, 0x88, 0x77, 0x66, 0x55,
            0x44, 0x33, 0x22, 0x11, 0x00,
          ]),
        },
        families: new Map(),
        family: vi.fn(() => ({})),
        operationAdvertised: vi.fn(() => false),
      },
      transport: { status: "connected", lastError: null },
      native: {
        supports: vi.fn(() => false),
        channel: null,
        extension: null,
        fs: null,
        git: null,
        kv: null,
        lsp: null,
      },
      desktopMedia: null,
      generation: 0,
      sessions: new Map(),
      focusedSessionId: null,
      snapshot: { ready: false },
      store: { isReady: () => false },
      listeners: new Set(),
    });

    (connection as unknown as { refreshSnapshot(): void }).refreshSnapshot();

    expect(
      (
        connection as unknown as {
          snapshot: { bootGeneration: bigint | null };
        }
      ).snapshot.bootGeneration,
    ).toBe(0xffee_ddcc_bbaa_9988_7766_5544_3322_1100n);
  });

  it("refreshes support flags without constructing product clients", () => {
    const factories = {
      fs: vi.fn(() => ({ family: "fs" }) as never),
      git: vi.fn(() => ({ family: "git" }) as never),
      lsp: vi.fn(() => ({ family: "lsp" }) as never),
      kv: vi.fn(() => ({ family: "kv" }) as never),
      channel: vi.fn(() => ({ family: "channel" }) as never),
      extension: vi.fn(() => ({ family: "extension" }) as never),
    };
    const session = {
      ready: true,
      hello: null,
      family: vi.fn(() => ({ limits: [] })),
      operationAdvertised: vi.fn(() => false),
    };
    const native = new YasNativeProductFamilies(session as never, factories);
    const connection = Object.create(
      YasNativeWorkspaceConnection.prototype,
    ) as YasNativeWorkspaceConnection;
    Object.assign(connection as object, {
      id: "remote",
      session,
      transport: { status: "connected", lastError: null },
      native,
      desktopMedia: null,
      generation: 0,
      sessions: new Map(),
      focusedSessionId: null,
      snapshot: { ready: false },
      store: { isReady: () => false },
      listeners: new Set(),
      readyListeners: new Set(),
    });
    const refresh = () =>
      (
        connection as unknown as {
          refreshSnapshot(): void;
        }
      ).refreshSnapshot();

    refresh();
    refresh();

    for (const factory of Object.values(factories))
      expect(factory).not.toHaveBeenCalled();
    expect(
      (
        connection as unknown as {
          snapshot: {
            supportsKv: boolean;
            supportsFsSync: boolean;
            supportsGit: boolean;
            supportsLsp: boolean;
            supportsChannels: boolean;
            supportsExtensions: boolean;
          };
        }
      ).snapshot,
    ).toMatchObject({
      supportsKv: true,
      supportsFsSync: true,
      supportsGit: true,
      supportsLsp: true,
      supportsChannels: true,
      supportsExtensions: true,
    });
    expect(native.fs).not.toBeNull();
    expect(factories.fs).toHaveBeenCalledOnce();
    for (const [name, factory] of Object.entries(factories))
      if (name !== "fs") expect(factory).not.toHaveBeenCalled();
    native.dispose();
  });

  it("keeps an opaque high-bit Surface handle through native Selection drag", async () => {
    const surfaceId = 0xfedc_ba98_7654_3210n;
    let finishDrop!: () => void;
    const dropPending = new Promise<void>((resolve) => {
      finishDrop = resolve;
    });
    const selection = {
      dragBegin: vi.fn().mockResolvedValue({
        dragHandle: 0x8000_0000_0000_0011n,
        revision: 0x8000_0000_0000_0022n,
      }),
      dragEnter: vi.fn(),
      dragMotion: vi.fn(),
      dragLeave: vi.fn(),
      dragDrop: vi.fn(() => dropPending),
      dragCancel: vi.fn().mockResolvedValue(undefined),
    };
    const connection = Object.create(
      YasNativeWorkspaceConnection.prototype,
    ) as YasNativeWorkspaceConnection;
    Object.assign(connection as object, {
      selectionClient: selection,
      browserDrag: null,
      nextBrowserDragToken: 1,
    });

    connection.sendSurfaceDragEnter(
      surfaceId,
      12.5,
      7.25,
      ["application/octet-stream"],
      ["application/octet-stream"],
    );
    await flush();

    expect(selection.dragEnter).toHaveBeenCalledWith(
      expect.objectContaining({
        targetSurface: surfaceId,
        actions: YAS_SELECTION_ACTION_COPY,
      }),
    );

    connection.sendSurfaceDragDrop(surfaceId, 13, 8, [
      {
        mime: "application/octet-stream",
        name: "drop.bin",
        data: new Uint8Array([1, 2, 3]),
      },
    ]);
    await flush();

    expect(selection.dragMotion).toHaveBeenCalledWith(
      expect.objectContaining({ targetSurface: surfaceId }),
    );
    expect(selection.dragDrop).toHaveBeenCalledWith(
      0x8000_0000_0000_0011n,
      0x8000_0000_0000_0022n,
      expect.any(Uint8Array),
      YAS_SELECTION_ACTION_COPY,
      [expect.objectContaining({ tag: 1, required: true })],
    );
    finishDrop();
    await flush();
  });
});
