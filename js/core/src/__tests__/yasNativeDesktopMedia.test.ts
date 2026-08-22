import { describe, expect, it, vi } from "vitest";
import { YasClientCatalog, YasClientClient } from "../yas/client";
import { YasDesktopCatalog, YasDesktopClient } from "../yas/desktop";
import { YasFontCatalog, YasFontClient } from "../yas/font";
import {
  YAS_FAMILY_MEDIA,
  YAS_MEDIA_CODEC_OPUS,
  YAS_MEDIA_FRAME,
} from "../yas/generated";
import {
  YasMediaCatalog,
  YasMediaClient,
  encodeMediaFrame,
} from "../yas/media";
import { YasNativeDesktopClientLifecycle } from "../yas/nativeDesktopMedia";
import { YasSelectionCatalog, YasSelectionClient } from "../yas/selection";
import { YasStateSubscription } from "../yas/state";
import { YasSurfaceCatalog, YasSurfaceClient } from "../yas/surface";
import { YasTerminalCatalog, YasTerminalClient } from "../yas/terminal";

const flush = () => new Promise((resolve) => setTimeout(resolve, 0));

function lifecycleWith(
  desktop: object | null,
  client: object | null,
  media: object | null,
): YasNativeDesktopClientLifecycle {
  const reset = vi.fn();
  const lifecycle = Object.create(
    YasNativeDesktopClientLifecycle.prototype,
  ) as YasNativeDesktopClientLifecycle;
  Object.assign(lifecycle as object, {
    disposed: false,
    desktopGeneration: 0,
    mediaGeneration: 0,
    removeDesktop: null,
    removeClient: null,
    removeMedia: null,
    removePortal: null,
    removeFrame: null,
    removeFrameAck: null,
    removeStreamStatus: null,
    desktop,
    client,
    media,
    stopCapture: vi.fn(),
    sendAudioUnsubscribe: vi.fn(),
    options: {
      desktopStore: { setNativeController: vi.fn(), reset },
      mediaStore: { setNativeController: vi.fn(), reset },
      mprisStore: { setNativeController: vi.fn(), reset },
      audioPlayer: { reset },
    },
    clientListeners: new Set(),
    clientSnapshot: null,
    assets: new Map(),
    assetBytes: 0,
    playerArtwork: new Map(),
    playerArtworkBytes: 0,
  });
  return lifecycle;
}

describe("YasNativeDesktopClientLifecycle", () => {
  it("cleans WATCHes that settle after invalidated disposal", async () => {
    let resolveWatches!: () => void;
    const watchesSettled = new Promise<void>((resolve) => {
      resolveWatches = resolve;
    });
    const unwatchers = [
      vi.fn().mockRejectedValue(new Error("desktop session is not ready")),
      vi.fn().mockRejectedValue(new Error("client session is not ready")),
      vi.fn().mockRejectedValue(new Error("media session is not ready")),
    ];
    const client = (unwatch: ReturnType<typeof vi.fn>) => ({
      dispose: vi.fn(),
      catalog: {
        subscribe: vi.fn(() => vi.fn()),
        watch: vi.fn(() => watchesSettled),
        unwatch,
      },
    });
    const desktop = client(unwatchers[0]!);
    const nativeClient = client(unwatchers[1]!);
    const media = {
      ...client(unwatchers[2]!),
      onPortalRequest: vi.fn(() => vi.fn()),
      onFrame: vi.fn(() => vi.fn()),
      onFrameAck: vi.fn(() => vi.fn()),
      onStreamStatus: vi.fn(() => vi.fn()),
    };
    const lifecycle = lifecycleWith(desktop, nativeClient, media);

    const start = lifecycle.start();
    await Promise.resolve();
    lifecycle.dispose();
    lifecycle.dispose();
    resolveWatches();
    await start;
    await flush();

    expect(desktop.catalog.watch).toHaveBeenCalledOnce();
    expect(nativeClient.catalog.watch).toHaveBeenCalledOnce();
    expect(media.catalog.watch).toHaveBeenCalledOnce();
    for (const unwatch of unwatchers) expect(unwatch).toHaveBeenCalledTimes(2);
    expect(desktop.dispose).toHaveBeenCalledOnce();
    expect(nativeClient.dispose).toHaveBeenCalledOnce();
    expect(media.dispose).toHaveBeenCalledOnce();
  });

  it("cleans sibling WATCHes when bootstrap fails", async () => {
    const failure = new Error("client WATCH failed");
    const catalogue = (watch: ReturnType<typeof vi.fn>) => ({
      subscribe: vi.fn(() => vi.fn()),
      watch,
      unwatch: vi.fn().mockResolvedValue(undefined),
    });
    const desktop = {
      dispose: vi.fn(),
      catalog: catalogue(vi.fn().mockResolvedValue(undefined)),
    };
    const nativeClient = {
      dispose: vi.fn(),
      catalog: catalogue(vi.fn().mockRejectedValue(failure)),
    };
    const lifecycle = lifecycleWith(desktop, nativeClient, null);

    await expect(lifecycle.start()).rejects.toBe(failure);
    await flush();

    expect(desktop.catalog.unwatch).toHaveBeenCalledOnce();
    expect(nativeClient.catalog.unwatch).toHaveBeenCalledOnce();
  });

  it("propagates unexpected family descriptor failures", () => {
    const failure = new Error("corrupt family descriptor");
    const session = {
      family: vi.fn(() => {
        throw failure;
      }),
      operationAdvertised: vi.fn(),
    };

    expect(
      () =>
        new YasNativeDesktopClientLifecycle({
          session: session as never,
          desktopStore: {} as never,
          mediaStore: {} as never,
          mprisStore: {} as never,
          audioPlayer: {} as never,
        }),
    ).toThrow(failure);
  });

  it("removes low-level session callbacks across repeated client lifecycles", () => {
    type Listener = (event: {
      payload: Uint8Array;
      sensitive: boolean;
      datagram: boolean;
    }) => void;
    const invalidations = new Set<(value: { family?: number }) => void>();
    const events = new Map<string, Set<Listener>>();
    const connection = {
      options: {},
      transport: { addEventListener: vi.fn() },
      family: vi.fn(() => ({ limits: [] })),
      registerFamilyLimitValidator: vi.fn(),
      onInvalidation: vi.fn(
        (listener: (value: { family?: number }) => void) => {
          invalidations.add(listener);
          return () => invalidations.delete(listener);
        },
      ),
      onEvent: vi.fn((family: number, kind: number, listener: Listener) => {
        const key = `${family}/${kind}`;
        const listeners = events.get(key) ?? new Set<Listener>();
        events.set(key, listeners);
        listeners.add(listener);
        return () => {
          listeners.delete(listener);
          if (listeners.size === 0) events.delete(key);
        };
      }),
    };
    const payload = encodeMediaFrame({
      streamHandle: 1n,
      sequence: 1n,
      captureTime: 1n,
      presentationTime: 1n,
      codecVersion: YAS_MEDIA_CODEC_OPUS,
      flags: 0,
      fragmentIndex: 0,
      fragmentCount: 1,
      completeLength: 1,
      payload: new Uint8Array([1]),
    });

    for (let iteration = 0; iteration < 3; iteration++) {
      const desktop = new YasDesktopClient(connection as never);
      const client = new YasClientClient(connection as never);
      const media = new YasMediaClient(connection as never);
      const terminal = new YasTerminalClient(connection as never);
      const surface = new YasSurfaceClient(connection as never);
      const selection = new YasSelectionClient(connection as never);
      const font = new YasFontClient(connection as never);
      const onFrame = vi.fn();
      media.onFrame(onFrame);

      expect(invalidations.size).toBe(12);
      const mediaFrameListeners = events.get(
        `${YAS_FAMILY_MEDIA}/${YAS_MEDIA_FRAME}`,
      );
      expect(mediaFrameListeners?.size).toBe(1);
      for (const listener of mediaFrameListeners ?? [])
        listener({ payload, sensitive: false, datagram: false });
      expect(onFrame).toHaveBeenCalledOnce();

      desktop.dispose();
      client.dispose();
      media.dispose();
      terminal.dispose();
      surface.dispose();
      selection.dispose();
      font.dispose();
      expect(invalidations.size).toBe(1);
      expect(events.has(`${YAS_FAMILY_MEDIA}/${YAS_MEDIA_FRAME}`)).toBe(false);
    }
  });

  it("rejects catalogue snapshots disposed after WATCH but before STATE", async () => {
    const invalidations = new Set<(value: { family?: number }) => void>();
    const connection = {
      options: {},
      onInvalidation: vi.fn(
        (listener: (value: { family?: number }) => void) => {
          invalidations.add(listener);
          return () => invalidations.delete(listener);
        },
      ),
    };
    const unwatch = vi.fn().mockResolvedValue(undefined);
    const watch = vi
      .spyOn(YasStateSubscription, "watch")
      .mockResolvedValue({ active: true, unwatch } as never);
    const catalogues = [
      new YasClientCatalog(connection as never),
      new YasDesktopCatalog(connection as never),
      new YasMediaCatalog(connection as never),
      new YasTerminalCatalog(connection as never),
      new YasSurfaceCatalog(connection as never),
      new YasSelectionCatalog(connection as never),
      new YasFontCatalog(connection as never),
    ];

    for (const catalogue of catalogues) {
      const snapshot = catalogue.firstSnapshot();
      await flush();
      catalogue.dispose();
      await expect(snapshot).rejects.toThrow("catalogue is disposed");
    }

    expect(watch).toHaveBeenCalledTimes(catalogues.length);
    expect(unwatch).toHaveBeenCalledTimes(catalogues.length);
    expect(invalidations.size).toBe(0);
    watch.mockRestore();
  });

  it("deduplicates and cancels pending catalogue WATCHes", async () => {
    const invalidations = new Set<(value: { family?: number }) => void>();
    const connection = {
      options: {},
      onInvalidation: vi.fn(
        (listener: (value: { family?: number }) => void) => {
          invalidations.add(listener);
          return () => invalidations.delete(listener);
        },
      ),
    };
    const pending: Array<{
      resolve: (subscription: YasStateSubscription) => void;
      unwatch: ReturnType<typeof vi.fn>;
    }> = [];
    const watch = vi.spyOn(YasStateSubscription, "watch").mockImplementation(
      () =>
        new Promise<YasStateSubscription>((resolve) => {
          pending.push({
            resolve,
            unwatch: vi.fn().mockResolvedValue(undefined),
          });
        }),
    );
    const catalogues = [
      new YasClientCatalog(connection as never),
      new YasDesktopCatalog(connection as never),
      new YasMediaCatalog(connection as never),
      new YasTerminalCatalog(connection as never),
      new YasSurfaceCatalog(connection as never),
      new YasSelectionCatalog(connection as never),
      new YasFontCatalog(connection as never),
    ];
    const startWatches = () =>
      catalogues.map((catalogue) => {
        const first = catalogue.watch();
        const second = catalogue.watch();
        expect(second).toBe(first);
        return [first, second] as const;
      });

    const invalidated = startWatches();
    expect(watch).toHaveBeenCalledTimes(catalogues.length);
    const invalidationAssertions = invalidated.flatMap(([first, second]) => [
      expect(first).rejects.toThrow(/invalidated/),
      expect(second).rejects.toThrow(/invalidated/),
    ]);
    for (const invalidate of [...invalidations]) invalidate({});
    await Promise.all(invalidationAssertions);
    const invalidatedSubscriptions = pending.splice(0);
    for (const item of invalidatedSubscriptions)
      item.resolve({ active: true, unwatch: item.unwatch } as never);
    await flush();
    for (const item of invalidatedSubscriptions)
      expect(item.unwatch).toHaveBeenCalledOnce();

    const disposed = startWatches();
    expect(watch).toHaveBeenCalledTimes(catalogues.length * 2);
    const disposalAssertions = disposed.flatMap(([first, second]) => [
      expect(first).rejects.toThrow(/disposed/),
      expect(second).rejects.toThrow(/disposed/),
    ]);
    for (const catalogue of catalogues) catalogue.dispose();
    await Promise.all(disposalAssertions);
    const disposedSubscriptions = pending.splice(0);
    for (const item of disposedSubscriptions)
      item.resolve({ active: true, unwatch: item.unwatch } as never);
    await flush();
    for (const item of disposedSubscriptions)
      expect(item.unwatch).toHaveBeenCalledOnce();

    expect(invalidations.size).toBe(0);
    watch.mockRestore();
  });

  it("isolates catalogue subscribers from sibling delivery and UNWATCH", async () => {
    const connection = {
      options: {},
      onInvalidation: vi.fn(() => vi.fn()),
    };
    const unwatchers: Array<ReturnType<typeof vi.fn>> = [];
    const watch = vi
      .spyOn(YasStateSubscription, "watch")
      .mockImplementation(async () => {
        const unwatch = vi.fn().mockResolvedValue(undefined);
        unwatchers.push(unwatch);
        return { active: true, unwatch } as never;
      });
    const catalogues = [
      new YasClientCatalog(connection as never),
      new YasDesktopCatalog(connection as never),
      new YasMediaCatalog(connection as never),
      new YasTerminalCatalog(connection as never),
      new YasSurfaceCatalog(connection as never),
      new YasSelectionCatalog(connection as never),
      new YasFontCatalog(connection as never),
    ];

    for (const catalogue of catalogues) {
      const rejectedInitially = vi.fn(() => {
        throw new Error("initial subscriber failed");
      });
      const rejectedLater = vi
        .fn()
        .mockImplementationOnce(() => undefined)
        .mockImplementation(() => {
          throw new Error("later subscriber failed");
        });
      const sibling = vi.fn();
      expect(() => catalogue.subscribe(rejectedInitially)).not.toThrow();
      catalogue.subscribe(rejectedLater);
      catalogue.subscribe(sibling);

      await catalogue.watch();
      await expect(catalogue.unwatch()).resolves.toBeUndefined();

      expect(rejectedInitially).toHaveBeenCalledOnce();
      expect(rejectedLater).toHaveBeenCalledTimes(3);
      expect(sibling).toHaveBeenCalledTimes(3);
      catalogue.dispose();
    }

    expect(watch).toHaveBeenCalledTimes(catalogues.length);
    for (const unwatch of unwatchers) expect(unwatch).toHaveBeenCalledOnce();
    watch.mockRestore();
  });
});
