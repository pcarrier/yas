import { describe, expect, it, vi } from "vitest";
import {
  YAS_CLASS_EVENT,
  YAS_CLASS_REQUEST,
  YAS_FAMILY_SURFACE,
  YAS_FAMILY_TERMINAL,
  YAS_SELECTION_ACTION_COPY,
  YAS_SELECTION_OWNER_EXTERNAL,
  YAS_SELECTION_SLOT_CLIPBOARD,
  YAS_SURFACE_AXIS,
  YAS_SURFACE_AXIS_SOURCE_CONTINUOUS,
  YAS_SURFACE_AXIS_STOP_X,
  YAS_SURFACE_AXIS_STOP_Y,
  YAS_SURFACE_CODEC_AV1_V1,
  YAS_SURFACE_CODEC_H264_V1,
  YAS_SURFACE_CURSOR_CUSTOM,
  YAS_SURFACE_CURSOR_HIDDEN,
  YAS_SURFACE_FRAME_END_OF_STREAM,
  YAS_SURFACE_FRAME_KEYFRAME,
  YAS_SURFACE_KEY,
  YAS_SURFACE_KEY_STATE_PRESSED,
  YAS_SURFACE_KEY_STATE_REPEAT,
  YAS_SURFACE_MODIFIER_CAPS_LOCK,
  YAS_SURFACE_POINTER,
  YAS_SURFACE_POINTER_BUTTON_BACK,
  YAS_SURFACE_POINTER_BUTTON_FORWARD,
  YAS_SURFACE_POINTER_BUTTON_MIDDLE,
  YAS_SURFACE_POINTER_BUTTON_PRIMARY,
  YAS_SURFACE_POINTER_BUTTON_SECONDARY,
  YAS_SURFACE_PREEDIT,
  YAS_SURFACE_RESIZE_SCALE_120_EXTENSION,
  YAS_SURFACE_STATE,
  YAS_SURFACE_STATE_ACK,
  YAS_SURFACE_STATE_CURSOR_EXTENSION,
  YAS_SURFACE_STATE_MINIMUM_SIZE_EXTENSION,
  YAS_SURFACE_STATE_TEXT_INPUT_EXTENSION,
  YAS_SURFACE_STATE_TEXT_INPUT_REQUEST_REVISION_EXTENSION,
  YAS_SURFACE_TEXT_INPUT_ENABLED,
  YAS_SURFACE_TEXT,
  YAS_SURFACE_TOUCH,
  YAS_SURFACE_TOUCH_PHASE_DOWN,
  YAS_SURFACE_TOUCH_PHASE_MOVE,
  YAS_SURFACE_TOUCH_PHASE_UP,
  YAS_SURFACE_UNWATCH,
  YAS_SURFACE_WATCH,
  YAS_STATUS_RESOURCE_EXHAUSTED,
  YAS_TERMINAL_COPY_RANGE,
  YAS_TERMINAL_STATE,
  YAS_TERMINAL_STATE_ACK,
  YAS_TERMINAL_UNWATCH,
  YAS_TERMINAL_WATCH,
} from "../yas/generated";
import {
  YasNativeWorkspaceConnection,
  customSurfaceCursorCss,
} from "../YasNativeWorkspaceConnection";
import * as YasSurfaceCanvas from "../YasSurfaceCanvas";
import { YasNativeProductFamilies } from "../yas/nativeProductFamilies";
import { encodeSurfaceCodecPayload } from "../yas/packed";
import { YasReceiveBudget } from "../yas/session";
import {
  YasCursor,
  YasProtocolError,
  YasResultError,
  YasWriter,
} from "../yas/wire";
import { CODEC_SUPPORT_AV1, CODEC_SUPPORT_H264 } from "../surfaceModel";
import { SurfaceStore } from "../SurfaceStore";
import { YasSurfaceClient, type YasSurfaceView } from "../yas/surface";

const flush = () => new Promise((resolve) => setTimeout(resolve, 0));

it("sends authoritative Caps Lock snapshots and retains them for synthetic releases", () => {
  const key = vi.fn();
  const connection = Object.create(
    YasNativeWorkspaceConnection.prototype,
  ) as YasNativeWorkspaceConnection;
  Object.assign(connection, {
    surface: { key },
    surfaceViews: new Map([
      [
        1n,
        {
          view: { result: { maxInflightFrames: 4 } },
          lastPresented: 0n,
          decoderQueueDepth: 0,
        },
      ],
    ]),
    pressedSurfaceKeys: new Set(),
    surfaceCapsLock: false,
    supportsSurfaceEvent: () => true,
  });

  connection.sendSurfaceInput(1n, 58, true, 10, true);
  connection.sendSurfaceInput(1n, 58, true, 11, true);
  connection.sendSurfaceInput(1n, 58, false);
  expect(key.mock.calls.map((call) => call[2].modifiers)).toEqual([
    YAS_SURFACE_MODIFIER_CAPS_LOCK,
    YAS_SURFACE_MODIFIER_CAPS_LOCK,
    YAS_SURFACE_MODIFIER_CAPS_LOCK,
  ]);
  expect(key.mock.calls[0][2].state).toBe(YAS_SURFACE_KEY_STATE_PRESSED);
  expect(key.mock.calls[1][2].state).toBe(YAS_SURFACE_KEY_STATE_REPEAT);

  // The OS toggled Caps Lock outside this view. No local toggle history is
  // needed, and a synthetic release must preserve this latest snapshot.
  connection.sendSurfaceInput(1n, 37, true, 12, false);
  connection.sendSurfaceInput(1n, 37, false);
  expect(key.mock.calls.slice(-2).map((call) => call[2].modifiers)).toEqual([
    0, 0,
  ]);
});

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
    resize: vi.fn().mockResolvedValue(1n),
  };
  const connection = Object.create(
    YasNativeWorkspaceConnection.prototype,
  ) as YasNativeWorkspaceConnection;
  Object.assign(connection as object, {
    disposed: false,
    pendingSurfaceViews: new Map(),
    surfaceViewRetryTimers: new Map(),
    surfaceViewRetryAttempts: new Map(),
    surfaceViewRetryForceReset: new Map(),
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
    listeners: new Set(),
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
      applySurfaceCatalog(records: readonly unknown[]): void;
    },
  };
}

describe("YasNativeWorkspaceConnection", () => {
  it("reserves a host clipboard write until the Wayland selection arrives", async () => {
    const originalClipboard = Object.getOwnPropertyDescriptor(
      navigator,
      "clipboard",
    );
    const originalClipboardItem = Object.getOwnPropertyDescriptor(
      globalThis,
      "ClipboardItem",
    );
    let written = "";
    class PendingClipboardItem {
      constructor(readonly values: Record<string, Promise<Blob>>) {}
    }
    const write = vi.fn(async (items: PendingClipboardItem[]) => {
      const blob = await items[0]!.values["text/plain"]!;
      written = await blob.text();
    });
    Object.defineProperty(globalThis, "ClipboardItem", {
      configurable: true,
      value: PendingClipboardItem,
    });
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { write, writeText: vi.fn() },
    });

    try {
      const listeners = new Set<() => void>();
      const get = vi.fn().mockResolvedValue({
        bytes: () => Promise.resolve(new TextEncoder().encode("from guest")),
      });
      const connection = Object.create(
        YasNativeWorkspaceConnection.prototype,
      ) as YasNativeWorkspaceConnection;
      Object.assign(connection as object, {
        listeners,
        selectionSlots: [
          {
            slot: YAS_SELECTION_SLOT_CLIPBOARD,
            ownerKind: YAS_SELECTION_OWNER_EXTERNAL,
            revision: 4n,
            mimeTypes: ["text/plain;charset=utf-8"],
          },
        ],
        selectionClient: { get },
        subscribe: (listener: () => void) => {
          listeners.add(listener);
          return () => listeners.delete(listener);
        },
      });

      connection.copyWaylandClipboardToHost();
      expect(write).toHaveBeenCalledOnce();
      expect(written).toBe("");

      (
        connection as unknown as {
          selectionSlots: unknown[];
        }
      ).selectionSlots = [
        {
          slot: YAS_SELECTION_SLOT_CLIPBOARD,
          ownerKind: YAS_SELECTION_OWNER_EXTERNAL,
          revision: 5n,
          mimeTypes: ["text/plain;charset=utf-8"],
        },
      ];
      for (const listener of listeners) listener();
      await write.mock.results[0]!.value;

      expect(get).toHaveBeenCalledWith({
        target: {
          kind: "slot",
          slot: YAS_SELECTION_SLOT_CLIPBOARD,
          revision: 5n,
        },
        mime: "text/plain;charset=utf-8",
      });
      expect(written).toBe("from guest");
    } finally {
      if (originalClipboard)
        Object.defineProperty(navigator, "clipboard", originalClipboard);
      else Reflect.deleteProperty(navigator, "clipboard");
      if (originalClipboardItem)
        Object.defineProperty(
          globalThis,
          "ClipboardItem",
          originalClipboardItem,
        );
      else Reflect.deleteProperty(globalThis, "ClipboardItem");
    }
  });

  it("focuses only the target and deduplicates an unresolved transaction", async () => {
    const focus = vi.fn().mockResolvedValue(1n);
    const connection = Object.create(
      YasNativeWorkspaceConnection.prototype,
    ) as YasNativeWorkspaceConnection;
    Object.assign(connection as object, {
      surface: { focus },
      surfaceRecords: new Map([
        [1n, {}],
        [2n, {}],
      ]),
      focusedSurfaceId: null,
      pendingSurfaceFocusId: null,
      surfaceFocusRequest: 0,
    });

    connection.sendSurfaceFocus(1n);
    expect(focus).toHaveBeenCalledOnce();
    expect(focus).toHaveBeenLastCalledWith(1n, expect.any(Uint8Array), true);
    connection.sendSurfaceFocus(1n);
    expect(focus).toHaveBeenCalledOnce();

    await Promise.resolve();
    connection.sendSurfaceFocus(1n);
    expect(focus).toHaveBeenCalledTimes(2);
    expect(focus).toHaveBeenLastCalledWith(1n, expect.any(Uint8Array), true);

    await Promise.resolve();
    connection.sendSurfaceFocus(2n);
    expect(focus).toHaveBeenCalledTimes(3);
    expect(focus).toHaveBeenLastCalledWith(2n, expect.any(Uint8Array), true);
  });

  it("allows Surface focus to retry after a rejected transaction", async () => {
    const failed = deferred<bigint>();
    const focus = vi
      .fn()
      .mockReturnValueOnce(failed.promise)
      .mockResolvedValue(1n);
    const connection = Object.create(
      YasNativeWorkspaceConnection.prototype,
    ) as YasNativeWorkspaceConnection;
    Object.assign(connection as object, {
      surface: { focus },
      surfaceRecords: new Map([[1n, {}]]),
      focusedSurfaceId: null,
      pendingSurfaceFocusId: null,
      surfaceFocusRequest: 0,
    });

    connection.sendSurfaceFocus(1n);
    connection.sendSurfaceFocus(1n);
    expect(focus).toHaveBeenCalledOnce();

    failed.reject(new Error("focus failed"));
    await Promise.resolve();
    await Promise.resolve();
    connection.sendSurfaceFocus(1n);
    expect(focus).toHaveBeenCalledTimes(2);
  });

  it("does not refresh a Surface view for an unchanged encode target", () => {
    const refresh = vi.fn();
    const connection = Object.create(
      YasNativeWorkspaceConnection.prototype,
    ) as YasNativeWorkspaceConnection;
    Object.assign(connection as object, {
      surfaceMounts: new Map([
        [1n, new Map([["view", { target: null, maxFps: 0 }]])],
      ]),
      requestNativeSurfaceViewRefresh: refresh,
    });

    // Live views repeatedly observe their present box, but null/uncapped is
    // still the same target and must not create another RESIZE transaction.
    connection.setSurfaceViewTarget(1n, "view", null, 0);
    connection.setSurfaceViewTarget(1n, "view", null);
    expect(refresh).not.toHaveBeenCalled();

    connection.setSurfaceViewTarget(
      1n,
      "view",
      { width: 512, height: 256 },
      12,
    );
    expect(refresh).toHaveBeenCalledOnce();
    refresh.mockClear();

    // ResizeObserver produces fresh objects, so equality is by dimensions.
    connection.setSurfaceViewTarget(
      1n,
      "view",
      { width: 512, height: 256 },
      12,
    );
    expect(refresh).not.toHaveBeenCalled();
  });

  it("does not refresh Surface views when an FPS cap normalizes unchanged", () => {
    const refresh = vi.fn();
    const connection = Object.create(
      YasNativeWorkspaceConnection.prototype,
    ) as YasNativeWorkspaceConnection;
    Object.assign(connection as object, {
      surfaceMaxFps: 60,
      surfaceMounts: new Map([[1n, new Map()]]),
      requestNativeSurfaceViewRefresh: refresh,
    });

    // Workspace snapshots reapply this preference for unrelated focus,
    // catalogue, and RTT changes. Equivalent values are inert.
    connection.setSurfaceMaxFpsCap(60);
    connection.setSurfaceMaxFpsCap(60.2);
    expect(refresh).not.toHaveBeenCalled();

    connection.setSurfaceMaxFpsCap(30);
    expect(refresh).toHaveBeenCalledOnce();
    expect(refresh).toHaveBeenCalledWith(1n);

    refresh.mockClear();
    connection.setSurfaceMaxFpsCap(30.4);
    expect(refresh).not.toHaveBeenCalled();
  });

  it("preserves standard DOM mouse buttons in native Surface pointer events", () => {
    const pointer = vi.fn();
    const view = { result: { maxInflightFrames: 4 } };
    const connection = Object.create(
      YasNativeWorkspaceConnection.prototype,
    ) as YasNativeWorkspaceConnection;
    Object.assign(connection as object, {
      session: { operationAdvertised: vi.fn(() => true) },
      surface: { pointer },
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
    });

    for (const button of [0, 1, 2, 3, 4]) {
      connection.sendSurfacePointer(1n, 0, button, 0.25, 0.75, 1);
    }

    expect(pointer.mock.calls.map((call) => call[2].button)).toEqual([
      YAS_SURFACE_POINTER_BUTTON_PRIMARY,
      YAS_SURFACE_POINTER_BUTTON_MIDDLE,
      YAS_SURFACE_POINTER_BUTTON_SECONDARY,
      YAS_SURFACE_POINTER_BUTTON_BACK,
      YAS_SURFACE_POINTER_BUTTON_FORWARD,
    ]);
  });

  it("uses only the focused surface as the A/V video clock", () => {
    const noteVideoPresentation = vi.fn();
    const connection = Object.create(
      YasNativeWorkspaceConnection.prototype,
    ) as YasNativeWorkspaceConnection;
    Object.assign(connection as object, {
      focusedSurfaceId: 2n,
      audioPlayer: { noteVideoPresentation },
    });
    const timing = connection as unknown as {
      noteSurfacePresentation(
        surfaceId: bigint,
        sourceMs: number,
        clientMs: number,
      ): void;
    };

    timing.noteSurfacePresentation(1n, 100, 200);
    timing.noteSurfacePresentation(2n, 110, 230);

    expect(noteVideoPresentation).toHaveBeenCalledTimes(1);
    expect(noteVideoPresentation).toHaveBeenCalledWith(110, 230);
  });

  it("turns custom Wayland cursor artwork into a valid scaled CSS cursor", () => {
    expect(customSurfaceCursorCss("blob:cursor", 4, 5, 240)).toBe(
      'image-set(url("blob:cursor") 2x) 4 5, url("blob:cursor") 8 10, default',
    );
  });

  it("keeps a 1x Wayland cursor as an ordinary CSS cursor", () => {
    expect(customSurfaceCursorCss("blob:cursor", 4, 5, 120)).toBe(
      'url("blob:cursor") 4 5, default',
    );
  });

  it("does not rebuild an unchanged custom cursor from a full Surface snapshot", () => {
    const createObjectURL = vi
      .spyOn(URL, "createObjectURL")
      .mockReturnValueOnce("blob:cursor-1")
      .mockReturnValueOnce("blob:cursor-2");
    const handleSurfaceCursor = vi.fn();
    const connection = Object.create(
      YasNativeWorkspaceConnection.prototype,
    ) as YasNativeWorkspaceConnection;
    Object.assign(connection as object, {
      surfaceRecords: new Map(),
      surfaceMounts: new Map(),
      surfaceViews: new Map(),
      surfaceStore: {
        handleSurfaceCreated: vi.fn(),
        handleSurfaceParent: vi.fn(),
        handleSurfaceResized: vi.fn(),
        handleSurfaceDestroyed: vi.fn(),
        handleSurfaceTitle: vi.fn(),
        handleSurfaceAppId: vi.fn(),
        handleSurfaceActivated: vi.fn(),
        handleSurfaceCursor,
        handleSurfaceTextInput: vi.fn(),
      },
      emit: vi.fn(),
    });
    const lifecycle = connection as unknown as {
      applySurfaceCatalog(records: readonly unknown[]): void;
    };
    const cursor = (png: Uint8Array) =>
      new YasWriter()
        .u8(YAS_SURFACE_CURSOR_CUSTOM)
        .bytes(new Uint8Array(3))
        .i32(4)
        .i32(5)
        .u32(16)
        .u32(16)
        .u16(120)
        .u16(0)
        .bytesU32(png)
        .finish();
    const record = (png: Uint8Array) => ({
      surfaceHandle: 1n,
      revision: 1n,
      parentHandle: 0n,
      appHandle: 0n,
      lifecycle: 0,
      compositeWidth: 640,
      compositeHeight: 480,
      logicalWidth32_32: 640n << 32n,
      logicalHeight32_32: 480n << 32n,
      applicationId: "app",
      title: "title",
      extensions: [
        {
          tag: YAS_SURFACE_STATE_CURSOR_EXTENSION,
          required: false,
          value: cursor(png),
        },
      ],
    });

    try {
      lifecycle.applySurfaceCatalog([record(new Uint8Array([1, 2, 3]))]);
      lifecycle.applySurfaceCatalog([record(new Uint8Array([1, 2, 3]))]);

      expect(createObjectURL).toHaveBeenCalledOnce();
      expect(handleSurfaceCursor).toHaveBeenCalledOnce();

      lifecycle.applySurfaceCatalog([record(new Uint8Array([4, 5, 6]))]);
      expect(createObjectURL).toHaveBeenCalledTimes(2);
      expect(handleSurfaceCursor).toHaveBeenCalledTimes(2);
    } finally {
      createObjectURL.mockRestore();
    }
  });

  it("delivers fresh keyboard requests once and clears withdrawn text input", () => {
    const { connection, lifecycle } = surfaceTestConnection(vi.fn());
    const store = new SurfaceStore();
    Object.assign(connection, {
      surfaceRecords: new Map(),
      surfaceMounts: new Map(),
      surfaceStore: store,
    });
    const delivered = vi.fn();
    store.onTextInput(delivered);
    const record = (revision: bigint | null, title = "title") => ({
      surfaceHandle: 1n,
      revision: 1n,
      parentHandle: 0n,
      appHandle: 0n,
      lifecycle: 0,
      compositeWidth: 640,
      compositeHeight: 480,
      logicalWidth32_32: 640n << 32n,
      logicalHeight32_32: 480n << 32n,
      applicationId: "app",
      title,
      extensions:
        revision === null
          ? []
          : [
              {
                tag: YAS_SURFACE_STATE_TEXT_INPUT_EXTENSION,
                required: false,
                value: new YasWriter()
                  .u16(YAS_SURFACE_TEXT_INPUT_ENABLED)
                  .u16(0)
                  .u32(6)
                  .u32(0)
                  .finish(),
              },
              {
                tag: YAS_SURFACE_STATE_TEXT_INPUT_REQUEST_REVISION_EXTENSION,
                required: false,
                value: new YasWriter().u64(revision).finish(),
              },
            ],
    });
    lifecycle.applySurfaceCatalog([record(3n)]);
    expect(delivered).toHaveBeenLastCalledWith(
      1n,
      expect.objectContaining({ enabled: true, requested: false }),
    );
    delivered.mockClear();
    // A title/video catalogue update must not replay the previous enable.
    lifecycle.applySurfaceCatalog([record(3n, "playing video")]);
    expect(delivered).not.toHaveBeenCalled();
    // Repeated enable of the same field survives coalescing with caret updates.
    lifecycle.applySurfaceCatalog([record(5n)]);
    expect(delivered).toHaveBeenCalledOnce();
    expect(delivered).toHaveBeenLastCalledWith(
      1n,
      expect.objectContaining({ enabled: true, requested: true }),
    );
    lifecycle.applySurfaceCatalog([record(5n)]);
    expect(delivered).toHaveBeenCalledOnce();
    lifecycle.applySurfaceCatalog([record(null)]);
    expect(delivered).toHaveBeenLastCalledWith(
      1n,
      expect.objectContaining({ enabled: false, requested: false }),
    );
    expect(store.getTextInput(1n)).toBeNull();
    lifecycle.applySurfaceCatalog([record(6n)]);
    expect(delivered).toHaveBeenLastCalledWith(
      1n,
      expect.objectContaining({ enabled: true, requested: true }),
    );
  });

  it("restores the default cursor when a snapshot withdraws hidden cursor state", () => {
    const { connection, lifecycle } = surfaceTestConnection(vi.fn());
    const store = new SurfaceStore();
    Object.assign(connection, {
      surfaceRecords: new Map(),
      surfaceMounts: new Map(),
      surfaceStore: store,
    });
    const canvas = document.createElement("canvas");
    store.onCursor((_, shape) => {
      canvas.style.cursor = shape;
    });
    const record = {
      surfaceHandle: 1n,
      revision: 1n,
      parentHandle: 0n,
      appHandle: 0n,
      lifecycle: 0,
      compositeWidth: 640,
      compositeHeight: 480,
      logicalWidth32_32: 640n << 32n,
      logicalHeight32_32: 480n << 32n,
      applicationId: "app",
      title: "title",
      extensions: [
        {
          tag: YAS_SURFACE_STATE_CURSOR_EXTENSION,
          required: false,
          value: new YasWriter()
            .u8(YAS_SURFACE_CURSOR_HIDDEN)
            .bytes(new Uint8Array(3))
            .finish(),
        },
      ],
    };
    try {
      lifecycle.applySurfaceCatalog([record]);
      expect(canvas.style.cursor).toBe("none");
      // Unrelated snapshots must preserve an app's intentional hide.
      lifecycle.applySurfaceCatalog([{ ...record, title: "changed" }]);
      expect(canvas.style.cursor).toBe("none");
      lifecycle.applySurfaceCatalog([{ ...record, extensions: [] }]);
      expect(store.getCursor(1n)).toBe("default");
      expect(canvas.style.cursor).toBe("default");
      // The next explicit hide must still work after withdrawing metadata.
      lifecycle.applySurfaceCatalog([record]);
      expect(canvas.style.cursor).toBe("none");
    } finally {
      store.destroy();
    }
  });

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
      surfaceViewRetryTimers: new Map(),
      surfaceViewRetryAttempts: new Map(),
      surfaceViewRetryForceReset: new Map(),
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
      configure: vi.fn().mockResolvedValue(undefined),
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
      viewSizes: Map<bigint, Map<string, { rows: number; cols: number }>>;
    };

    const first = lifecycle.openView(1n);
    const second = lifecycle.openView(1n);
    expect(openView).toHaveBeenCalledOnce();
    lifecycle.viewSizes.set(1n, new Map([["pane", { rows: 50, cols: 140 }]]));
    result.resolve(view);
    await Promise.all([first, second]);

    expect(lifecycle.views.size).toBe(1);
    expect(lifecycle.pendingViews.size).toBe(0);
    expect(view.configure).toHaveBeenCalledWith({ rows: 50, cols: 140 });
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

  it("coalesces refreshes arriving in flight into one latest-state rerun", async () => {
    const firstResize = deferred<bigint>();
    const { connection, lifecycle } = surfaceTestConnection(vi.fn());
    const native = connection as unknown as {
      surface: { resize: ReturnType<typeof vi.fn> };
      surfaceViewSizes: Map<
        bigint,
        Map<
          string,
          { width: number; height: number; scale120: number; request: number }
        >
      >;
    };
    native.surface.resize
      .mockReturnValueOnce(firstResize.promise)
      .mockResolvedValue(2n);
    native.surfaceViewSizes.set(
      1n,
      new Map([
        ["view", { width: 800, height: 600, scale120: 120, request: 1 }],
      ]),
    );
    const view = surfaceTestView(YAS_SURFACE_CODEC_H264_V1);
    lifecycle.surfaceViews.set(1n, {
      view,
      removeFrames: vi.fn(),
      width: 640,
      height: 480,
      maxFps: 120,
      lastReceived: 0n,
      lastPresented: 0n,
      decoderQueueDepth: 0,
    });

    const first = lifecycle.refreshNativeSurfaceView(1n);
    expect(native.surface.resize).toHaveBeenCalledOnce();
    expect(view.configure).toHaveBeenCalledOnce();

    native.surfaceViewSizes.set(
      1n,
      new Map([
        ["view", { width: 960, height: 720, scale120: 120, request: 2 }],
      ]),
    );
    const followers = [
      lifecycle.refreshNativeSurfaceView(1n),
      lifecycle.refreshNativeSurfaceView(1n),
      lifecycle.refreshNativeSurfaceView(1n),
    ];
    expect(native.surface.resize).toHaveBeenCalledOnce();

    firstResize.resolve(1n);
    await Promise.all([first, ...followers]);

    expect(native.surface.resize).toHaveBeenCalledTimes(2);
    expect(native.surface.resize).toHaveBeenLastCalledWith(
      1n,
      expect.any(Uint8Array),
      960n << 32n,
      720n << 32n,
      expect.any(Array),
    );
    expect(view.configure).toHaveBeenCalledTimes(2);
    expect(view.configure).toHaveBeenLastCalledWith(
      expect.objectContaining({ width: 960, height: 720 }),
    );
  });

  it.each(["before", "after"])(
    "preserves the final opening resize when catalogue geometry arrives %s the offer",
    async (order) => {
      const firstResize = deferred<bigint>();
      const view = surfaceTestView(YAS_SURFACE_CODEC_H264_V1);
      const openView = vi.fn().mockResolvedValue(view);
      const { connection, lifecycle } = surfaceTestConnection(openView);
      const native = connection as unknown as {
        surface: { resize: ReturnType<typeof vi.fn> };
      };
      const record = {
        surfaceHandle: 1n,
        revision: 1n,
        parentHandle: 0n,
        logicalWidth32_32: 640n << 32n,
        logicalHeight32_32: 480n << 32n,
        compositeWidth: 640,
        compositeHeight: 480,
        extensions: [],
      };
      Object.assign(connection, {
        surfaceRecords: new Map([[1n, record]]),
        surfaceStore: {
          handleSurfaceResized: vi.fn(),
          handleSurfaceEncoder: vi.fn(),
        },
        emit: vi.fn(),
      });
      native.surface.resize
        .mockReturnValueOnce(firstResize.promise)
        .mockResolvedValue(2n);

      connection.offerSurfaceViewSize(1n, "view", 800, 600, 120);
      const reconciliation = lifecycle.pendingSurfaceViews.get(1n)!.promise;
      const publishGeometry = () =>
        lifecycle.applySurfaceCatalog([
          {
            ...record,
            revision: 2n,
            logicalWidth32_32: 800n << 32n,
            logicalHeight32_32: 600n << 32n,
            compositeWidth: 800,
            compositeHeight: 600,
          },
        ]);
      if (order === "before") publishGeometry();
      connection.offerSurfaceViewSize(1n, "view", 960, 720, 120);
      connection.offerSurfaceViewSize(1n, "view", 1024, 768, 120);
      if (order === "after") publishGeometry();
      expect(native.surface.resize).toHaveBeenCalledTimes(3);
      expect(native.surface.resize).toHaveBeenLastCalledWith(
        1n,
        expect.any(Uint8Array),
        1024n << 32n,
        768n << 32n,
        expect.any(Array),
      );
      expect(openView).not.toHaveBeenCalled();

      firstResize.resolve(1n);
      await reconciliation;

      expect(native.surface.resize).toHaveBeenCalledTimes(3);
      expect(native.surface.resize).toHaveBeenLastCalledWith(
        1n,
        expect.any(Uint8Array),
        1024n << 32n,
        768n << 32n,
        expect.any(Array),
      );
      expect(openView).toHaveBeenCalledOnce();
      expect(openView).toHaveBeenCalledWith(
        expect.objectContaining({ width: 1024, height: 768 }),
      );
    },
  );

  it.each(["opening", "queued", "retrying"])(
    "retries a failed resize after a catalogue refresh while %s",
    async (phase) => {
      const queued = phase === "queued";
      vi.useFakeTimers();
      try {
        const failedResize = deferred<bigint>();
        const configured = deferred<void>();
        const view = surfaceTestView(YAS_SURFACE_CODEC_H264_V1);
        const { connection, lifecycle } = surfaceTestConnection(
          vi.fn().mockResolvedValue(view),
        );
        const native = connection as unknown as {
          surface: { resize: ReturnType<typeof vi.fn> };
        };
        const record = {
          surfaceHandle: 1n,
          revision: 1n,
          parentHandle: 0n,
          logicalWidth32_32: 640n << 32n,
          logicalHeight32_32: 480n << 32n,
          compositeWidth: 640,
          compositeHeight: 480,
          extensions: [],
        };
        Object.assign(connection, {
          surfaceRecords: new Map([[1n, record]]),
          surfaceStore: {
            handleSurfaceResized: vi.fn(),
            handleSurfaceEncoder: vi.fn(),
          },
          emit: vi.fn(),
        });
        if (queued) {
          await lifecycle.refreshNativeSurfaceView(1n);
          view.configure.mockReturnValueOnce(configured.promise);
          native.surface.resize.mockResolvedValueOnce(1n);
          connection.offerSurfaceViewSize(1n, "view", 800, 600, 120);
        }
        native.surface.resize
          .mockReturnValueOnce(failedResize.promise)
          .mockResolvedValue(3n);
        connection.offerSurfaceViewSize(1n, "view", 1024, 768, 120);
        const publishGeometry = () =>
          lifecycle.applySurfaceCatalog([
            {
              ...record,
              revision: 2n,
              logicalWidth32_32: 800n << 32n,
              logicalHeight32_32: 600n << 32n,
              compositeWidth: 800,
              compositeHeight: 600,
            },
          ]);
        if (phase !== "retrying") publishGeometry();
        failedResize.reject(new Error("temporary resize failure"));
        configured.resolve();
        if (phase === "retrying") {
          // The failed attempt has already scheduled its backoff. A later
          // catalogue update must retain that geometry work when it cancels
          // the timer to refresh the stream immediately.
          await vi.advanceTimersByTimeAsync(0);
          publishGeometry();
        }
        await vi.advanceTimersByTimeAsync(100);

        expect(native.surface.resize).toHaveBeenCalledTimes(queued ? 3 : 2);
        expect(native.surface.resize).toHaveBeenLastCalledWith(
          1n,
          expect.any(Uint8Array),
          1024n << 32n,
          768n << 32n,
          expect.any(Array),
        );
        expect(lifecycle.surfaceViews.has(1n)).toBe(true);
      } finally {
        vi.useRealTimers();
      }
    },
  );

  it("keeps sending latest RESIZE geometry while CONFIGURE is unresolved", async () => {
    const configuredFirst = deferred<void>();
    const configuredLatest = deferred<void>();
    const { connection, lifecycle } = surfaceTestConnection(vi.fn());
    const native = connection as unknown as {
      surface: { resize: ReturnType<typeof vi.fn> };
    };
    const view = surfaceTestView(YAS_SURFACE_CODEC_H264_V1);
    view.configure
      .mockReturnValueOnce(configuredFirst.promise)
      .mockReturnValueOnce(configuredLatest.promise);
    lifecycle.surfaceViews.set(1n, {
      view,
      removeFrames: vi.fn(),
      width: 640,
      height: 480,
      maxFps: 120,
      lastReceived: 0n,
      lastPresented: 0n,
      decoderQueueDepth: 0,
    });

    connection.offerSurfaceViewSize(1n, "view", 800, 600, 120);
    expect(native.surface.resize).toHaveBeenCalledOnce();
    expect(view.configure).toHaveBeenCalledOnce();

    connection.offerSurfaceViewSize(1n, "view", 960, 720, 120);
    connection.offerSurfaceViewSize(1n, "view", 1024, 768, 120);

    // Every already-throttled geometry offer reaches the server immediately;
    // encoder CONFIGURE remains one-in-flight and latest-only.
    expect(native.surface.resize).toHaveBeenCalledTimes(3);
    expect(native.surface.resize).toHaveBeenNthCalledWith(
      2,
      1n,
      expect.any(Uint8Array),
      960n << 32n,
      720n << 32n,
      expect.any(Array),
    );
    expect(native.surface.resize).toHaveBeenNthCalledWith(
      3,
      1n,
      expect.any(Uint8Array),
      1024n << 32n,
      768n << 32n,
      expect.any(Array),
    );
    expect(view.configure).toHaveBeenCalledOnce();

    const reconciliation = lifecycle.pendingSurfaceViews.get(1n)!.promise;
    configuredFirst.resolve();
    await vi.waitFor(() => expect(view.configure).toHaveBeenCalledTimes(2));
    expect(view.configure).toHaveBeenLastCalledWith(
      expect.objectContaining({ width: 1024, height: 768 }),
    );
    expect(lifecycle.surfaceViews.get(1n)).toMatchObject({
      // The first success remains the real encoder size until the latest
      // serialized CONFIGURE settles; it must not become the final state.
      width: 800,
      height: 600,
    });

    configuredLatest.resolve();
    await reconciliation;
    expect(lifecycle.surfaceViews.get(1n)).toMatchObject({
      width: 1024,
      height: 768,
    });
    expect(native.surface.resize).toHaveBeenCalledTimes(3);
    expect(view.configure).toHaveBeenCalledTimes(2);
  });

  it("sends pane geometry while OPEN_VIEW is waiting for an encoder", async () => {
    const opened = deferred<ReturnType<typeof surfaceTestView>>();
    const openView = vi.fn().mockReturnValue(opened.promise);
    const { connection, lifecycle } = surfaceTestConnection(openView);
    const native = connection as unknown as {
      surface: { resize: ReturnType<typeof vi.fn> };
    };
    const reconciliation = lifecycle.refreshNativeSurfaceView(1n);
    await vi.waitFor(() => expect(openView).toHaveBeenCalledOnce());
    native.surface.resize.mockClear();

    connection.offerSurfaceViewSize(1n, "view", 960, 720, 120);
    expect(native.surface.resize).toHaveBeenCalledWith(
      1n,
      expect.any(Uint8Array),
      960n << 32n,
      720n << 32n,
      expect.any(Array),
    );
    expect(lifecycle.surfaceViews.has(1n)).toBe(false);

    opened.resolve(surfaceTestView(YAS_SURFACE_CODEC_H264_V1));
    await reconciliation;
    expect(lifecycle.surfaceViews.get(1n)).toMatchObject({
      width: 960,
      height: 720,
    });
  });

  it("pipelines an unchanged-view RESET behind RESIZE", async () => {
    const resized = deferred<bigint>();
    const reset = deferred<void>();
    const order: string[] = [];
    const frameReady = vi.fn();
    const { connection, lifecycle } = surfaceTestConnection(vi.fn());
    const native = connection as unknown as {
      surface: { resize: ReturnType<typeof vi.fn> };
    };
    native.surface.resize.mockImplementation(() => {
      order.push("resize");
      return resized.promise;
    });
    const view = surfaceTestView(YAS_SURFACE_CODEC_H264_V1);
    view.reset.mockImplementation(() => {
      order.push("reset");
      return reset.promise;
    });
    lifecycle.surfaceViews.set(1n, {
      view,
      removeFrames: vi.fn(),
      width: 640,
      height: 480,
      maxFps: 120,
      lastReceived: 0n,
      lastPresented: 0n,
      decoderQueueDepth: 0,
    });

    connection.offerSurfaceViewSize(
      1n,
      "view",
      640,
      480,
      120,
      undefined,
      frameReady,
    );

    // RESET is written immediately after RESIZE, without waiting for its
    // round trip, but the presentation boundary joins both results.
    expect(order).toEqual(["resize", "reset"]);
    expect(frameReady).not.toHaveBeenCalled();

    resized.resolve(1n);
    await Promise.resolve();
    expect(frameReady).not.toHaveBeenCalled();

    reset.resolve();
    await lifecycle.pendingSurfaceViews.get(1n)!.promise;
    expect(frameReady).toHaveBeenCalledOnce();
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

  it("retries a mounted Surface view after a transient OPEN_VIEW failure", async () => {
    vi.useFakeTimers();
    try {
      const view = surfaceTestView(YAS_SURFACE_CODEC_H264_V1);
      const openView = vi
        .fn()
        .mockRejectedValueOnce(new Error("temporary Surface admission failure"))
        .mockResolvedValueOnce(view);
      const { connection, lifecycle } = surfaceTestConnection(openView);

      connection.refreshSurfaceSubscribe(1n);
      await vi.waitFor(() => expect(openView).toHaveBeenCalledOnce());
      expect(lifecycle.surfaceViews.has(1n)).toBe(false);

      await vi.advanceTimersByTimeAsync(100);
      await vi.waitFor(() => expect(openView).toHaveBeenCalledTimes(2));
      expect(lifecycle.surfaceViews.has(1n)).toBe(true);
    } finally {
      vi.useRealTimers();
    }
  });

  it("preserves a joined forced reset across a transient refresh failure", async () => {
    vi.useFakeTimers();
    try {
      const failedResize = deferred<bigint>();
      const { connection, lifecycle } = surfaceTestConnection(vi.fn());
      const native = connection as unknown as {
        surface: { resize: ReturnType<typeof vi.fn> };
        surfaceViewSizes: Map<
          bigint,
          Map<
            string,
            { width: number; height: number; scale120: number; request: number }
          >
        >;
      };
      native.surface.resize
        .mockReturnValueOnce(failedResize.promise)
        .mockResolvedValue(2n);
      native.surfaceViewSizes.set(
        1n,
        new Map([
          ["view", { width: 640, height: 480, scale120: 120, request: 1 }],
        ]),
      );
      const view = surfaceTestView(YAS_SURFACE_CODEC_H264_V1);
      lifecycle.surfaceViews.set(1n, {
        view,
        removeFrames: vi.fn(),
        width: 640,
        height: 480,
        maxFps: 120,
        lastReceived: 0n,
        lastPresented: 0n,
        decoderQueueDepth: 0,
      });

      connection.offerSurfaceViewSize(1n, "view", 640, 480, 120);
      expect(native.surface.resize).toHaveBeenCalledOnce();
      connection.refreshSurfaceSubscribe(1n);
      failedResize.reject(new Error("temporary resize failure"));
      await Promise.resolve();
      await Promise.resolve();

      await vi.advanceTimersByTimeAsync(100);
      await vi.waitFor(() =>
        expect(native.surface.resize).toHaveBeenCalledTimes(2),
      );
      await vi.waitFor(() => expect(view.reset).toHaveBeenCalledOnce());
    } finally {
      vi.useRealTimers();
    }
  });

  it("retries the desired Surface size instead of leaving a window scaled down", async () => {
    vi.useFakeTimers();
    try {
      const view = surfaceTestView(YAS_SURFACE_CODEC_H264_V1);
      const openView = vi.fn().mockResolvedValue(view);
      const { connection, lifecycle } = surfaceTestConnection(openView);
      const native = connection as unknown as {
        surface: { resize: ReturnType<typeof vi.fn> };
        surfaceViewSizes: Map<
          bigint,
          Map<string, { width: number; height: number; scale120: number }>
        >;
      };
      const resize = native.surface.resize;
      resize
        .mockRejectedValueOnce(new Error("temporary Surface resize failure"))
        .mockResolvedValueOnce(2n);
      native.surfaceViewSizes.set(
        1n,
        new Map([["view", { width: 1600, height: 1200, scale120: 240 }]]),
      );

      connection.refreshSurfaceSubscribe(1n);
      await vi.waitFor(() => expect(resize).toHaveBeenCalledOnce());
      expect(openView).not.toHaveBeenCalled();

      await vi.advanceTimersByTimeAsync(100);
      await vi.waitFor(() => expect(resize).toHaveBeenCalledTimes(2));
      expect(openView).toHaveBeenCalledOnce();
      expect(lifecycle.surfaceViews.has(1n)).toBe(true);
    } finally {
      vi.useRealTimers();
    }
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

  it.each([true, false])(
    "strips Surface codec metadata and does not decode EOS (frame geometry: %s)",
    (frameGeometry) => {
      const handleSurfaceFrame = vi.fn();
      const connection = Object.create(
        YasNativeWorkspaceConnection.prototype,
      ) as YasNativeWorkspaceConnection;
      Object.assign(connection as object, {
        surfaceStore: { handleSurfaceFrame },
        surfaceRecords: new Map([
          [
            1n,
            { logicalWidth32_32: 800n << 32n, logicalHeight32_32: 600n << 32n },
          ],
        ]),
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
        dimensions: { width: 424, height: 302 },
        ...(frameGeometry
          ? { logicalDimensions: { width: 400, height: 300 } }
          : {}),
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
      expect(handleSurfaceFrame.mock.calls[0]?.[3]).toBe(424);
      expect(handleSurfaceFrame.mock.calls[0]?.[4]).toBe(302);
      expect(handleSurfaceFrame.mock.calls[0]?.[5]).toEqual(bitstream);
      expect(handleSurfaceFrame.mock.calls[0]?.[9]).toEqual(
        frameGeometry
          ? { width: 400, height: 300 }
          : { width: 800, height: 600 },
      );

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
    },
  );

  it("sends logical Surface dimensions with the measured 2x scale", () => {
    const resize = vi.fn();
    const connection = Object.create(
      YasNativeWorkspaceConnection.prototype,
    ) as YasNativeWorkspaceConnection;
    Object.assign(connection as object, {
      surface: { resize },
      surfaceRecords: new Map([[1n, {}]]),
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

  it("intersects portrait and landscape view bounds independently of DPI", () => {
    const resize = vi.fn();
    const connection = Object.create(
      YasNativeWorkspaceConnection.prototype,
    ) as YasNativeWorkspaceConnection;
    Object.assign(connection, {
      surface: { resize },
      surfaceRecords: new Map([[1n, {}]]),
      surfaceMounts: new Map(),
      surfaceViewSizes: new Map(),
      session: { ready: true },
    });
    connection.offerSurfaceViewSize(1n, "desktop", 1600, 900, 120);
    connection.offerSurfaceViewSize(1n, "portrait", 1080, 2340, 360);
    connection.offerSurfaceViewSize(1n, "landscape", 1560, 720, 240);
    // Width from the portrait phone, height from the landscape one; neither
    // selecting one viewer nor taking physical minima fits all three clients.
    expect(resize).toHaveBeenLastCalledWith(
      1n,
      expect.any(Uint8Array),
      360n << 32n,
      360n << 32n,
      [
        expect.objectContaining({
          tag: YAS_SURFACE_RESIZE_SCALE_120_EXTENSION,
          value: new Uint8Array([104, 1]),
        }),
      ],
    );
    connection.withdrawSurfaceViewSize(1n, "portrait");
    expect(resize).toHaveBeenLastCalledWith(
      1n,
      expect.any(Uint8Array),
      780n << 32n,
      360n << 32n,
      [
        expect.objectContaining({
          tag: YAS_SURFACE_RESIZE_SCALE_120_EXTENSION,
          value: new Uint8Array([240, 0]),
        }),
      ],
    );
  });

  it.each([
    {
      width: 1080,
      height: 2340,
      scale120: 360,
      minWidth: 500,
      minHeight: 0,
      logicalWidth: 500,
      logicalHeight: 1083,
    },
    {
      width: 2400,
      height: 1200,
      scale120: 360,
      minWidth: 0,
      minHeight: 500,
      logicalWidth: 1000,
      logicalHeight: 500,
    },
    {
      width: 800,
      height: 1600,
      scale120: 240,
      minWidth: 500,
      minHeight: 1200,
      logicalWidth: 600,
      logicalHeight: 1200,
    },
  ])(
    "reclaims logical space from a $minWidth×$minHeight minimum",
    async ({
      width,
      height,
      scale120,
      minWidth,
      minHeight,
      logicalWidth,
      logicalHeight,
    }) => {
      const openView = vi
        .fn()
        .mockResolvedValue(surfaceTestView(YAS_SURFACE_CODEC_AV1_V1));
      const { connection, lifecycle } = surfaceTestConnection(openView);
      const internal = connection as any;
      internal.surfaceRecords.get(1n).extensions = [
        {
          tag: YAS_SURFACE_STATE_MINIMUM_SIZE_EXTENSION,
          required: false,
          value: new YasWriter().u32(minWidth).u32(minHeight).finish(),
        },
      ];
      internal.surfaceViewSizes.set(
        1n,
        new Map([["view", { width, height, scale120 }]]),
      );
      await lifecycle.refreshNativeSurfaceView(1n);
      expect(internal.surface.resize).toHaveBeenLastCalledWith(
        1n,
        expect.any(Uint8Array),
        BigInt(logicalWidth) << 32n,
        BigInt(logicalHeight) << 32n,
        expect.any(Array),
      );
      // More application content, not more decode pixels.
      expect(openView).toHaveBeenLastCalledWith(
        expect.objectContaining({ width, height }),
      );
    },
  );

  it("reclaims minimum-forced space per mount, and retracts it when hints are released", async () => {
    const view = surfaceTestView(YAS_SURFACE_CODEC_AV1_V1);
    const { connection, lifecycle } = surfaceTestConnection(
      vi.fn().mockResolvedValue(view),
    );
    const internal = connection as any;
    const record = {
      surfaceHandle: 1n,
      revision: 1n,
      parentHandle: 0n,
      logicalWidth32_32: 400n << 32n,
      logicalHeight32_32: 800n << 32n,
      compositeWidth: 1200,
      compositeHeight: 2400,
      extensions: [],
    };
    internal.surfaceRecords.set(1n, record);
    internal.surfaceViewSizes.set(
      1n,
      new Map([
        ["phone", { width: 1200, height: 2400, scale120: 360 }],
        ["desktop", { width: 1600, height: 900, scale120: 120 }],
      ]),
    );
    internal.surfaceStore.handleSurfaceResized = vi.fn();
    internal.emit = vi.fn();
    await lifecycle.refreshNativeSurfaceView(1n);
    internal.surface.resize.mockClear();
    const constrained = {
      ...record,
      revision: 2n,
      extensions: [
        {
          tag: YAS_SURFACE_STATE_MINIMUM_SIZE_EXTENSION,
          required: false,
          value: new YasWriter().u32(500).u32(0).finish(),
        },
      ],
    };
    // Minima can arrive without any change to the currently rendered size.
    lifecycle.applySurfaceCatalog([constrained]);
    await vi.waitFor(() => expect(lifecycle.pendingSurfaceViews.size).toBe(0));
    expect(internal.surface.resize).toHaveBeenLastCalledWith(
      1n,
      expect.any(Uint8Array),
      500n << 32n,
      900n << 32n,
      expect.any(Array),
    );
    internal.surface.resize.mockClear();
    lifecycle.applySurfaceCatalog([
      {
        ...constrained,
        revision: 3n,
        logicalWidth32_32: 500n << 32n,
        logicalHeight32_32: 900n << 32n,
        compositeWidth: 1500,
        compositeHeight: 2700,
      },
    ]);
    await vi.waitFor(() => expect(lifecycle.pendingSurfaceViews.size).toBe(0));
    expect(internal.surface.resize).not.toHaveBeenCalled();
    lifecycle.applySurfaceCatalog([{ ...record, revision: 4n }]);
    await vi.waitFor(() => expect(lifecycle.pendingSurfaceViews.size).toBe(0));
    expect(internal.surface.resize).toHaveBeenLastCalledWith(
      1n,
      expect.any(Uint8Array),
      400n << 32n,
      800n << 32n,
      expect.any(Array),
    );
  });

  it("preserves sub-1x logical dimensions at the Wayland 1x floor", () => {
    const resize = vi.fn();
    const connection = Object.create(
      YasNativeWorkspaceConnection.prototype,
    ) as YasNativeWorkspaceConnection;
    Object.assign(connection as object, {
      surface: { resize },
      surfaceRecords: new Map([[1n, {}]]),
      surfaceMounts: new Map(),
      surfaceViewSizes: new Map(),
      session: { ready: true },
    });

    expect(connection.offerSurfaceViewSize(1n, "pane", 800, 600, 60)).toBe(
      true,
    );
    expect(resize).toHaveBeenCalledWith(
      1n,
      expect.any(Uint8Array),
      1600n << 32n,
      1200n << 32n,
      [
        {
          tag: YAS_SURFACE_RESIZE_SCALE_120_EXTENSION,
          required: true,
          value: new Uint8Array([120, 0]),
        },
      ],
    );
  });

  it("acknowledges a view resize only after its catalogue revision is applied", async () => {
    const resize = vi.fn().mockResolvedValue(7n);
    const applied = vi.fn();
    const connection = Object.create(
      YasNativeWorkspaceConnection.prototype,
    ) as YasNativeWorkspaceConnection;
    Object.assign(connection as object, {
      surface: { resize },
      surfaceRecords: new Map([[1n, {}]]),
      surfaceMounts: new Map(),
      surfaceViewSizes: new Map(),
      surfaceCatalogRevision: 5n,
      pendingSurfaceResizeApplied: [],
      familyInitializationEpoch: 1,
      session: { ready: true },
    });

    expect(
      connection.offerSurfaceViewSize(1n, "pane", 1200, 900, 120, applied),
    ).toBe(true);
    await Promise.resolve();
    await Promise.resolve();
    expect(applied).not.toHaveBeenCalled();

    const lifecycle = connection as unknown as {
      noteSurfaceCatalogRevision(revision: bigint): void;
    };
    lifecycle.noteSurfaceCatalogRevision(6n);
    expect(applied).not.toHaveBeenCalled();
    lifecycle.noteSurfaceCatalogRevision(7n);
    expect(applied).toHaveBeenCalledOnce();
  });

  it("marks the replacement-frame boundary after Surface view reconfiguration", async () => {
    const order: string[] = [];
    const resized = deferred<bigint>();
    const configured = deferred<void>();
    const { connection, lifecycle } = surfaceTestConnection(vi.fn());
    const native = connection as unknown as {
      surface: { resize: ReturnType<typeof vi.fn> };
      surfaceViewSizes: Map<bigint, Map<string, unknown>>;
      surfaceCatalogRevision: bigint;
      pendingSurfaceResizeApplied: unknown[];
      familyInitializationEpoch: number;
    };
    native.surfaceCatalogRevision = 1n;
    native.pendingSurfaceResizeApplied = [];
    native.familyInitializationEpoch = 1;
    native.surface.resize.mockImplementation(() => {
      order.push("resize");
      return resized.promise;
    });
    native.surfaceViewSizes.set(
      1n,
      new Map([
        [
          "view",
          {
            width: 400,
            height: 300,
            scale120: 120,
            request: 1,
            onApplied: () => order.push("catalogue"),
            onFrameReady: () => order.push("frame-ready"),
          },
        ],
      ]),
    );
    const view = surfaceTestView(YAS_SURFACE_CODEC_H264_V1);
    view.configure.mockImplementation(() => {
      order.push("configure");
      return configured.promise;
    });
    lifecycle.surfaceViews.set(1n, {
      view,
      removeFrames: vi.fn(),
      width: 1200,
      height: 900,
      maxFps: 120,
      lastReceived: 0n,
      lastPresented: 0n,
      decoderQueueDepth: 0,
    });

    const updating = lifecycle.refreshNativeSurfaceView(1n);

    // Reliable requests are written in order without waiting for RESIZE's
    // result, removing one round trip from an existing-view resize.
    expect(order).toEqual(["resize", "configure"]);
    const state = lifecycle.surfaceViews.get(1n) as {
      width: number;
      height: number;
    };
    expect(state).toMatchObject({ width: 1200, height: 900 });

    resized.resolve(1n);
    await Promise.resolve();
    await Promise.resolve();
    expect(order).toEqual(["resize", "configure", "catalogue"]);
    expect(state).toMatchObject({ width: 1200, height: 900 });

    configured.resolve();
    await updating;

    expect(view.configure).toHaveBeenCalledWith(
      expect.objectContaining({ width: 400, height: 300 }),
    );
    expect(state).toMatchObject({ width: 400, height: 300 });
    expect(order).toEqual(["resize", "configure", "catalogue", "frame-ready"]);
  });

  it("publishes successful CONFIGURE dimensions when pipelined RESIZE fails", async () => {
    const { connection, lifecycle } = surfaceTestConnection(vi.fn());
    const native = connection as unknown as {
      surface: { resize: ReturnType<typeof vi.fn> };
      surfaceViewSizes: Map<
        bigint,
        Map<
          string,
          { width: number; height: number; scale120: number; request: number }
        >
      >;
    };
    native.surface.resize.mockRejectedValueOnce(
      new Error("Surface RESIZE failed"),
    );
    native.surfaceViewSizes.set(
      1n,
      new Map([
        ["view", { width: 800, height: 600, scale120: 120, request: 1 }],
      ]),
    );
    const view = surfaceTestView(YAS_SURFACE_CODEC_H264_V1);
    lifecycle.surfaceViews.set(1n, {
      view,
      removeFrames: vi.fn(),
      width: 640,
      height: 480,
      maxFps: 120,
      lastReceived: 0n,
      lastPresented: 0n,
      decoderQueueDepth: 0,
    });

    await expect(lifecycle.refreshNativeSurfaceView(1n)).rejects.toThrow(
      "Surface RESIZE failed",
    );

    expect(view.configure).toHaveBeenCalledWith(
      expect.objectContaining({ width: 800, height: 600 }),
    );
    expect(lifecycle.surfaceViews.get(1n)).toMatchObject({
      width: 800,
      height: 600,
    });
  });

  it("releases the Surface size and DPI claim with its last resizable view", () => {
    const resize = vi.fn();
    const connection = Object.create(
      YasNativeWorkspaceConnection.prototype,
    ) as YasNativeWorkspaceConnection;
    Object.assign(connection as object, {
      surface: { resize },
      surfaceRecords: new Map([[1n, {}]]),
      surfaceMounts: new Map(),
      surfaceViewSizes: new Map(),
      session: { ready: true },
    });

    connection.offerSurfaceViewSize(1n, "pane", 1600, 1200, 240);
    resize.mockClear();
    connection.withdrawSurfaceViewSize(1n, "pane");

    expect(resize).toHaveBeenCalledWith(1n, expect.any(Uint8Array), 0n, 0n);
  });

  it("preserves rounded HiDPI Surface dimensions for pointer space", () => {
    const handleSurfaceCreated = vi.fn();
    const handleSurfaceParent = vi.fn();
    const handleSurfaceResized = vi.fn();
    const connection = Object.create(
      YasNativeWorkspaceConnection.prototype,
    ) as YasNativeWorkspaceConnection;
    Object.assign(connection as object, {
      surfaceRecords: new Map(),
      surfaceMounts: new Map(),
      surfaceStore: {
        handleSurfaceCreated,
        handleSurfaceParent,
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
      compositeWidth: 1598,
      compositeHeight: 1198,
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
      1598,
      1198,
      "title",
      "app",
    );
    expect(handleSurfaceResized).toHaveBeenCalledWith(1n, 1598, 1198, 800, 600);

    handleSurfaceResized.mockClear();
    lifecycle.applySurfaceCatalog([
      {
        ...record,
        parentHandle: 7n,
        compositeWidth: 800,
        compositeHeight: 600,
      },
    ]);
    expect(handleSurfaceParent).toHaveBeenCalledWith(1n, 7n);
    expect(handleSurfaceResized).toHaveBeenCalledWith(1n, 800, 600, 800, 600);
  });

  it("opens a desired Surface view at composite size before its pane is measured", async () => {
    const view = surfaceTestView(YAS_SURFACE_CODEC_AV1_V1);
    const openView = vi.fn().mockResolvedValue(view);
    const { connection, lifecycle } = surfaceTestConnection(openView);
    (
      connection as unknown as {
        surfaceRecords: Map<bigint, unknown>;
        surfaceStore: Record<string, unknown>;
      }
    ).surfaceRecords.clear();
    Object.assign(
      (
        connection as unknown as {
          surfaceStore: Record<string, unknown>;
        }
      ).surfaceStore,
      {
        handleSurfaceCreated: vi.fn(),
        handleSurfaceParent: vi.fn(),
        handleSurfaceResized: vi.fn(),
        handleSurfaceDestroyed: vi.fn(),
        handleSurfaceTitle: vi.fn(),
        handleSurfaceAppId: vi.fn(),
      },
    );

    lifecycle.applySurfaceCatalog([
      {
        surfaceHandle: 1n,
        revision: 1n,
        parentHandle: 0n,
        appHandle: 0n,
        lifecycle: 0,
        compositeWidth: 1280,
        compositeHeight: 960,
        logicalWidth32_32: 640n << 32n,
        logicalHeight32_32: 480n << 32n,
        applicationId: "brave-browser",
        title: "Brave",
        extensions: [],
      },
    ]);

    await vi.waitFor(() => expect(openView).toHaveBeenCalledOnce());
    expect(openView).toHaveBeenCalledWith(
      expect.objectContaining({ width: 1280, height: 960 }),
    );
    expect(lifecycle.surfaceViews.has(1n)).toBe(true);
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

  it.each([
    { scale120: 480, logicalWidth: 500, logicalHeight: 400 },
    { scale120: 960, logicalWidth: 500, logicalHeight: 400 },
    { scale120: 60, logicalWidth: 3000, logicalHeight: 2200 },
  ])(
    "bounds the stream by its pane when a $scale120-scale window exceeds it",
    async ({ scale120, logicalWidth, logicalHeight }) => {
      const view = surfaceTestView(YAS_SURFACE_CODEC_AV1_V1);
      const openView = vi.fn().mockResolvedValue(view);
      const { connection, lifecycle } = surfaceTestConnection(openView);
      Object.assign(connection, {
        surfaceViewSizes: new Map([
          [1n, new Map([["view", { width: 1200, height: 1000, scale120 }]])],
        ]),
        surfaceRecords: new Map([
          [
            1n,
            {
              surfaceHandle: 1n,
              revision: 1n,
              parentHandle: 0n,
              logicalWidth32_32: BigInt(logicalWidth) << 32n,
              logicalHeight32_32: BigInt(logicalHeight) << 32n,
              compositeWidth: (logicalWidth * Math.max(120, scale120)) / 120,
              compositeHeight: (logicalHeight * Math.max(120, scale120)) / 120,
              extensions: [],
            },
          ],
        ]),
      });
      await lifecycle.refreshNativeSurfaceView(1n);
      expect(openView).toHaveBeenCalledWith(
        expect.objectContaining({
          width: 1200,
          height: 1000,
        }),
      );
    },
  );

  it("keeps the stream bounded when the app publishes and releases a minimum size", async () => {
    const view = surfaceTestView(YAS_SURFACE_CODEC_AV1_V1);
    const openView = vi.fn().mockResolvedValue(view);
    const { connection, lifecycle } = surfaceTestConnection(openView);
    const record = {
      surfaceHandle: 1n,
      revision: 1n,
      parentHandle: 0n,
      logicalWidth32_32: 150n << 32n,
      logicalHeight32_32: 125n << 32n,
      compositeWidth: 1200,
      compositeHeight: 1000,
      extensions: [],
    };
    const handleSurfaceResized = vi.fn();
    Object.assign(connection, {
      surfaceViewSizes: new Map([
        [1n, new Map([["view", { width: 1200, height: 1000, scale120: 960 }]])],
      ]),
      surfaceRecords: new Map([[1n, record]]),
      surfaceStore: { handleSurfaceResized, handleSurfaceEncoder: vi.fn() },
      emit: vi.fn(),
    });
    await lifecycle.refreshNativeSurfaceView(1n);
    expect(openView).toHaveBeenCalledWith(
      expect.objectContaining({ width: 1200, height: 1000 }),
    );
    const resize = (
      connection as unknown as { surface: { resize: ReturnType<typeof vi.fn> } }
    ).surface.resize;
    resize.mockClear();
    lifecycle.applySurfaceCatalog([
      {
        ...record,
        revision: 2n,
        logicalWidth32_32: 500n << 32n,
        logicalHeight32_32: 400n << 32n,
        compositeWidth: 4000,
        compositeHeight: 3200,
      },
    ]);
    await vi.waitFor(() => expect(lifecycle.pendingSurfaceViews.size).toBe(0));
    expect(handleSurfaceResized).toHaveBeenLastCalledWith(
      1n,
      4000,
      3200,
      500,
      400,
    );
    expect(view.configure).not.toHaveBeenCalled();
    expect(lifecycle.surfaceViews.get(1n)).toMatchObject({
      width: 1200,
      height: 1000,
    });
    expect(resize).not.toHaveBeenCalled();
    lifecycle.applySurfaceCatalog([{ ...record, revision: 3n }]);
    await vi.waitFor(() => expect(lifecycle.pendingSurfaceViews.size).toBe(0));
    expect(handleSurfaceResized).toHaveBeenLastCalledWith(
      1n,
      1200,
      1000,
      150,
      125,
    );
    expect(view.configure).not.toHaveBeenCalled();
    expect(resize).not.toHaveBeenCalled();
  });

  it("opens a sub-1x Surface view at the viewer size", async () => {
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
      new Map([["view", { width: 800, height: 600, scale120: 60 }]]),
    );

    await lifecycle.refreshNativeSurfaceView(1n);

    expect(openView).toHaveBeenCalledWith(
      expect.objectContaining({ width: 800, height: 600, maxFps: 120 }),
    );
    expect(
      (
        connection as unknown as {
          surface: { resize: ReturnType<typeof vi.fn> };
        }
      ).surface.resize,
    ).toHaveBeenCalledWith(
      1n,
      expect.any(Uint8Array),
      1600n << 32n,
      1200n << 32n,
      [
        expect.objectContaining({
          tag: YAS_SURFACE_RESIZE_SCALE_120_EXTENSION,
          value: new Uint8Array([120, 0]),
        }),
      ],
    );
  });

  it("does not let a sub-1x claim undersize a default-scale viewer", async () => {
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
      new Map([
        ["default", { width: 800, height: 600, scale120: 0 }],
        ["downscaled", { width: 400, height: 300, scale120: 60 }],
      ]),
    );

    await lifecycle.refreshNativeSurfaceView(1n);

    expect(openView).toHaveBeenCalledWith(
      expect.objectContaining({ width: 800, height: 600, maxFps: 120 }),
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
    const firstTerminalSnapshot = vi.fn().mockResolvedValue(undefined);
    const removeCatalogs = [vi.fn(), vi.fn(), vi.fn(), vi.fn(), vi.fn()];
    const subscribe = vi.fn();
    for (const remove of removeCatalogs) subscribe.mockReturnValueOnce(remove);
    let surfaceWatchActive = false;
    const surfaceWireWatches = vi.fn();
    const surfaceUnwatch = vi.fn(async () => {
      surfaceWatchActive = false;
    });
    const firstSurfaceSnapshot = vi.fn(() => {
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
        firstSnapshot: firstSurfaceSnapshot,
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
        catalog: { subscribe, firstSnapshot: firstTerminalSnapshot },
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
      expect(firstTerminalSnapshot).toHaveBeenCalledOnce();
      await Promise.resolve();
      expect(surfaceWireWatches).toHaveBeenCalledOnce();
      expect(lifecycle.snapshot).toMatchObject({
        status: "connected",
        ready: false,
        error: null,
      });

      lifecycle.onSessionCatalogChange();
      await Promise.resolve();
      expect(firstTerminalSnapshot).toHaveBeenCalledOnce();
      expect(firstSurfaceSnapshot).toHaveBeenCalledOnce();

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

      expect(firstTerminalSnapshot).toHaveBeenCalledTimes(2);
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
      expect(firstTerminalSnapshot).toHaveBeenCalledTimes(3);
      expect(firstSurfaceSnapshot).toHaveBeenCalledTimes(3);
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
      expect(firstTerminalSnapshot).toHaveBeenCalledTimes(4);
      expect(firstSurfaceSnapshot).toHaveBeenCalledTimes(4);
      expect(surfaceWireWatches).toHaveBeenCalledTimes(2);
      expect(surfaceDispose).not.toHaveBeenCalled();
      expect(lifecycle.snapshot.generation).toBe(generationAfterHello + 1);

      firstTerminalSnapshot.mockRejectedValueOnce(currentInitializationError);
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
      expect(firstTerminalSnapshot).toHaveBeenCalledTimes(5);
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

  it("encodes signed iPad touch identifiers without losing contact identity", () => {
    const sendEvent = vi.fn();
    const surface = Object.create(
      YasSurfaceClient.prototype,
    ) as YasSurfaceClient;
    Object.assign(surface, { connection: { sendEvent } });
    const view = { result: { viewId: 7 } } as YasSurfaceView;
    const connection = Object.create(
      YasNativeWorkspaceConnection.prototype,
    ) as YasNativeWorkspaceConnection;
    Object.assign(connection, {
      surface,
      surfaceViews: new Map([[1n, { view }]]),
      surfaceTouchUsers: 1,
      session: { operationAdvertised: () => true },
    });
    const contacts = [
      { identifier: -2147483648, x: 10, y: 20 },
      { identifier: -1, x: 30, y: 40 },
      { identifier: 0, x: 50, y: 60 },
      { identifier: 2147483647, x: 70, y: 80 },
    ];
    for (const [phase, wirePhase] of [
      [0, YAS_SURFACE_TOUCH_PHASE_DOWN],
      [2, YAS_SURFACE_TOUCH_PHASE_MOVE],
      [1, YAS_SURFACE_TOUCH_PHASE_UP],
    ]) {
      connection.sendSurfaceTouch(1n, phase, contacts, 100);
      const [family, kind, payload] = sendEvent.mock.lastCall!;
      expect([family, kind]).toEqual([YAS_FAMILY_SURFACE, YAS_SURFACE_TOUCH]);
      const reader = new YasCursor(payload);
      expect(reader.u32()).toBe(7);
      expect(reader.u64()).toBe(100_000_000n);
      expect(reader.u8()).toBe(wirePhase);
      expect(reader.u8()).toBe(0);
      expect(reader.u16()).toBe(4);
      for (const [index, id] of [
        0x80000000, 0xffffffff, 0, 0x7fffffff,
      ].entries()) {
        expect(reader.u32()).toBe(id);
        expect(reader.i64()).toBe(BigInt(contacts[index].x) << 32n);
        expect(reader.i64()).toBe(BigInt(contacts[index].y) << 32n);
      }
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

    connection.sendSurfaceAxis2(1n, {
      dx: 12,
      dy: -24,
      v120x: 0,
      v120y: -120,
      source: YAS_SURFACE_AXIS_SOURCE_CONTINUOUS,
      stop: false,
      timeMs: 10,
    });
    expect(axis.mock.lastCall?.[2]).toMatchObject({
      clientMonotonicNs: 10_000_000n,
      source: YAS_SURFACE_AXIS_SOURCE_CONTINUOUS,
      flags: 0,
      stepsX: 0,
      stepsY: -1,
    });

    connection.sendSurfaceAxis2(1n, {
      dx: 0,
      dy: 0,
      v120x: 0,
      v120y: 0,
      source: YAS_SURFACE_AXIS_SOURCE_CONTINUOUS,
      stop: true,
    });
    expect(axis.mock.lastCall?.[2].flags).toBe(
      YAS_SURFACE_AXIS_STOP_X | YAS_SURFACE_AXIS_STOP_Y,
    );
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

  it.each(["text/plain", "image/png"])(
    "keeps native drag metadata and opaque Surface handles for %s",
    async (mime) => {
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
        mime === "image/png"
          ? ["text/uri-list", "application/octet-stream"]
          : [mime],
        mime === "image/png" ? [mime] : undefined,
      );
      await flush();

      expect(selection.dragBegin).toHaveBeenCalledWith(
        expect.any(Uint8Array),
        YAS_SELECTION_ACTION_COPY,
        [
          {
            name: mime === "image/png" ? "0.png" : "",
            mimeTypes: expect.arrayContaining([mime]),
          },
        ],
      );

      expect(selection.dragEnter).toHaveBeenCalledWith(
        expect.objectContaining({
          targetSurface: surfaceId,
          actions: YAS_SELECTION_ACTION_COPY,
        }),
      );

      connection.sendSurfaceDragDrop(surfaceId, 13, 8, [
        {
          mime,
          name: mime === "image/png" ? "0.png" : "",
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
    },
  );
});
