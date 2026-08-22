import { describe, expect, it, vi } from "vitest";
import type { ChannelOpenOptions } from "@yas-run/core";
import {
  openSession,
  SESSION_ARTWORK_READ_CREDIT,
  SESSION_ARTWORK_MAX_BYTES,
  SESSION_ARTWORK_MAX_ENTRIES,
  SESSION_ICON_QUEUE_MAX_BYTES,
  SESSION_ICON_QUEUE_MAX_ENTRIES,
  SESSION_ICON_REQUEST_MAX_ENTRIES,
  SESSION_MAX_ID_CHARS,
  SESSION_MAX_APPS,
  SESSION_MAX_CATALOG_ENTRIES,
  SESSION_MAX_NAME_CHARS,
  SESSION_MAX_STATE_BYTES,
  SessionMirror,
} from "../session";

const encode = (value: unknown): Uint8Array =>
  new TextEncoder().encode(JSON.stringify(value));

const state = (apps: unknown[], catalog?: unknown[]): Uint8Array =>
  encode(
    catalog === undefined
      ? { type: "state", apps }
      : { type: "state", apps, catalog },
  );

describe("SessionMirror", () => {
  it("is not ready until state arrives", () => {
    const mirror = new SessionMirror();
    expect(mirror.ready).toBe(false);
    expect(mirror.apps).toEqual([]);
    mirror.apply(state([]));
    expect(mirror.ready).toBe(true);
  });

  it("sorts by display name, not by id", () => {
    const mirror = new SessionMirror();
    mirror.apply(
      state([
        { id: "zed", name: "Alpha", enabled: true, phase: "running" },
        { id: "alpha", name: "Zed", enabled: true, phase: "running" },
      ]),
    );
    expect(mirror.apps.map((app) => app.id)).toEqual(["zed", "alpha"]);
  });

  /** The catalog is the larger half and rides only a greeting or a resync, so
   *  an ordinary update must not be read as "everything was uninstalled". */
  it("keeps the catalog across an update that omits it", () => {
    const mirror = new SessionMirror();
    mirror.apply(state([], [{ id: "a", name: "A" }]));
    expect(mirror.catalog).toHaveLength(1);
    mirror.apply(
      state([{ id: "a", name: "A", enabled: true, phase: "running" }]),
    );
    expect(mirror.catalog).toHaveLength(1);
    expect(mirror.apps).toHaveLength(1);
  });

  it("defaults missing fields rather than dropping the row", () => {
    const mirror = new SessionMirror();
    mirror.apply(state([{ id: "bare" }]));
    expect(mirror.apps[0]).toMatchObject({
      id: "bare",
      name: "bare",
      enabled: false,
      phase: "stopped",
      failures: 0,
      windows: 0,
    });
    expect(mirror.apps[0]?.socket).toBeUndefined();
  });

  it("drops rows with no id, and unknown phases fall back to stopped", () => {
    const mirror = new SessionMirror();
    mirror.apply(
      state([
        { id: "", name: "nameless" },
        { name: "no id at all" },
        { id: "ok", phase: "wat" },
      ]),
    );
    expect(mirror.apps.map((app) => app.id)).toEqual(["ok"]);
    expect(mirror.apps[0]?.phase).toBe("stopped");
  });

  /** A panel is not the place to surface a parser disagreement: a malformed
   *  message must leave the last good state standing. */
  it("ignores malformed payloads and foreign message types", () => {
    const mirror = new SessionMirror();
    mirror.apply(state([{ id: "keep", name: "Keep", phase: "running" }]));
    const revision = mirror.revision;

    mirror.apply(new TextEncoder().encode("not json at all"));
    mirror.apply(encode({ type: "hello" }));
    mirror.apply(encode([1, 2, 3]));
    mirror.apply(new Uint8Array());

    expect(mirror.apps.map((app) => app.id)).toEqual(["keep"]);
    expect(mirror.revision).toBe(revision);
  });

  /** Three states, not two: a row has to be able to tell "no artwork exists"
   *  from "the answer has not arrived", or it re-asks forever. The supervisor
   *  answers where the artwork is; the bytes are the panel's own errand. */
  it("records where an icon is, and records its absence too", () => {
    const mirror = new SessionMirror();
    expect(mirror.icon("gimp")).toBeUndefined();
    expect(mirror.path("gimp")).toBeUndefined();

    mirror.apply(
      encode({ type: "icon", id: "gimp", path: "/i/128x128/apps/gimp.png" }),
    );
    expect(mirror.path("gimp")).toBe("/i/128x128/apps/gimp.png");
    // Located, not yet read: the row still has no URL to draw.
    expect(mirror.icon("gimp")).toBeUndefined();
    expect(mirror.unread()).toEqual([
      { id: "gimp", path: "/i/128x128/apps/gimp.png" },
    ]);

    mirror.setIcon("gimp", "blob:whatever");
    expect(mirror.icon("gimp")).toBe("blob:whatever");
    expect(mirror.unread()).toEqual([]);

    // No `path` field is the answer "there is none", and it is final.
    mirror.apply(encode({ type: "icon", id: "bare" }));
    expect(mirror.path("bare")).toBeNull();
    expect(mirror.icon("bare")).toBeNull();
    expect(mirror.unread()).toEqual([]);
  });

  /** An icon message carries no apps, and reading it as state would empty the
   *  list every time a row's artwork arrived. */
  it("an icon message leaves the application list alone", () => {
    const mirror = new SessionMirror();
    mirror.apply(
      state(
        [{ id: "a", name: "A", phase: "running" }],
        [{ id: "a", name: "A" }],
      ),
    );
    mirror.apply(encode({ type: "icon", id: "a", path: "/i/a.svg" }));
    expect(mirror.apps.map((app) => app.id)).toEqual(["a"]);
    expect(mirror.catalog).toHaveLength(1);
  });

  /** A path is a path: anything that is not a non-empty string is the answer
   *  "there is nothing to draw", never something handed to a reader. */
  it("refuses a path that is not one", () => {
    const mirror = new SessionMirror();
    mirror.apply(encode({ type: "icon", id: "a", path: 42 }));
    expect(mirror.path("a")).toBeNull();
    expect(mirror.icon("a")).toBeNull();
    mirror.apply(encode({ type: "icon", id: "b", path: "" }));
    expect(mirror.icon("b")).toBeNull();
    // An id-less message answers for nothing and is dropped whole.
    mirror.apply(encode({ type: "icon", path: "/i/x.png" }));
    expect(mirror.icon("")).toBeUndefined();
  });

  it("notifies subscribers once per applied message", () => {
    const mirror = new SessionMirror();
    let calls = 0;
    const stop = mirror.subscribe(() => calls++);
    mirror.apply(state([]));
    mirror.apply(state([{ id: "a", phase: "running" }]));
    expect(calls).toBe(2);
    stop();
    mirror.apply(state([]));
    expect(calls).toBe(2);
  });

  it("bounds hostile state by rows and retained bytes", () => {
    const mirror = new SessionMirror();
    mirror.apply(
      state(
        Array.from({ length: SESSION_MAX_APPS + 50 }, (_, index) => ({
          id: `app-${index}`,
          name: "a".repeat(SESSION_MAX_NAME_CHARS * 2),
        })),
        Array.from(
          { length: SESSION_MAX_CATALOG_ENTRIES + 50 },
          (_, index) => ({
            id: `catalog-${index}`,
            name: "c".repeat(SESSION_MAX_NAME_CHARS * 2),
          }),
        ),
      ),
    );

    const stats = mirror.stateStats();
    expect(stats.apps).toBeLessThanOrEqual(SESSION_MAX_APPS);
    expect(stats.catalog).toBeLessThanOrEqual(SESSION_MAX_CATALOG_ENTRIES);
    expect(stats.bytes).toBeLessThanOrEqual(SESSION_MAX_STATE_BYTES);
    expect(
      mirror.apps.every((app) => app.name.length <= SESSION_MAX_NAME_CHARS),
    ).toBe(true);
    expect(
      mirror.catalog.every(
        (entry) => entry.name.length <= SESSION_MAX_NAME_CHARS,
      ),
    ).toBe(true);
  });

  it("LRU-bounds artwork and revokes evicted and reconciled blob URLs", () => {
    const revoked: string[] = [];
    const mirror = new SessionMirror({
      onIconEvicted: (url) => revoked.push(url),
    });
    const ids = Array.from(
      { length: SESSION_ARTWORK_MAX_ENTRIES + 8 },
      (_, index) => `app-${index}`,
    );
    mirror.apply(
      state(
        [],
        ids.map((id) => ({ id, name: id })),
      ),
    );
    for (const id of ids) mirror.setIcon(id, `blob:${id}`, 1);

    expect(mirror.cacheStats().entries).toBe(SESSION_ARTWORK_MAX_ENTRIES);
    expect(mirror.cacheStats().bytes).toBeLessThanOrEqual(
      SESSION_ARTWORK_MAX_BYTES,
    );
    expect(mirror.icon(ids[0] ?? "")).toBeUndefined();
    expect(revoked).toContain(`blob:${ids[0]}`);

    const kept = ids.at(-1) ?? "";
    mirror.apply(state([], [{ id: kept, name: kept }]));
    expect(mirror.cacheStats().entries).toBe(1);
    expect(mirror.icon(kept)).toBe(`blob:${kept}`);
    mirror.dispose();
    expect(mirror.cacheStats()).toEqual({ entries: 0, bytes: 0 });
    expect(revoked).toContain(`blob:${kept}`);
  });

  it("enforces the artwork byte cap independently of the item cap", () => {
    const mirror = new SessionMirror();
    const ids = Array.from({ length: 40 }, (_, index) => `large-${index}`);
    mirror.apply(
      state(
        [],
        ids.map((id) => ({ id, name: id })),
      ),
    );
    for (const id of ids) mirror.setIcon(id, `blob:${id}`, 1024 * 1024);
    expect(mirror.cacheStats().entries).toBeLessThan(ids.length);
    expect(mirror.cacheStats().bytes).toBeLessThanOrEqual(
      SESSION_ARTWORK_MAX_BYTES,
    );
  });
});

/**
 * The four verbs are two pairs, and the wire is the only place the difference
 * is expressed: `stop` leaves intent alone, `disable` does not. A panel that
 * sent the wrong one would look right and quietly forget an application.
 */
describe("openSession", () => {
  const fakeConnection = (initiallyWritable = true) => {
    const sent: string[] = [];
    let writable = initiallyWritable;
    // Captured so a test can answer, which is the only way to reach the mirror
    // the way the wire does.
    const inbound: {
      deliver?: (payload: Uint8Array) => void;
      credit?: (available: bigint) => void;
      close?: (reason: number, detail: string) => void;
    } = {};
    return {
      sent,
      inbound,
      connectChannel: async (
        _name: string,
        options: ChannelOpenOptions = {},
      ) => {
        inbound.deliver = options.onData;
        inbound.credit = options.onCredit;
        inbound.close = options.onClosed;
        return {
          id: 2,
          send: (payload: string | Uint8Array) => {
            if (!writable) return false;
            sent.push(
              typeof payload === "string"
                ? payload
                : new TextDecoder().decode(payload),
            );
            return true;
          },
          close: () => {},
        };
      },
      setWritable: (next: boolean) => {
        writable = next;
        if (next) inbound.credit?.(1024n);
      },
    } as unknown as {
      sent: string[];
      inbound: {
        deliver?: (payload: Uint8Array) => void;
        credit?: (available: bigint) => void;
        close?: (reason: number, detail: string) => void;
      };
      setWritable(next: boolean): void;
    } & Parameters<typeof openSession>[0];
  };

  it("keeps the first icon batch queued until channel credit arrives", async () => {
    vi.useFakeTimers();
    try {
      const connection = fakeConnection(false);
      const session = await openSession(connection);
      session.requestIcons(["brave-browser"]);
      vi.advanceTimersByTime(0);
      expect(connection.sent).toEqual([]);

      connection.setWritable(true);
      vi.advanceTimersByTime(0);
      expect(connection.sent).toEqual(["icons brave-browser"]);

      connection.sent.length = 0;
      session.requestIcons(["brave-browser"]);
      vi.advanceTimersByTime(0);
      expect(connection.sent).toEqual([]);
      session.close();
    } finally {
      vi.useRealTimers();
    }
  });

  it("keeps launcher commands queued until channel credit arrives", async () => {
    const connection = fakeConnection(false);
    const session = await openSession(connection);

    session.start("spotify");
    expect(connection.sent).toEqual([]);

    connection.setWritable(true);
    expect(connection.sent).toEqual(["start spotify"]);
    session.close();
  });

  it("sends one line per verb, naming the application", async () => {
    const connection = fakeConnection();
    const session = await openSession(connection);
    session.enable("org.gnome.Nautilus");
    session.disable("org.gnome.Nautilus");
    session.start("org.gnome.Nautilus");
    session.stop("org.gnome.Nautilus");
    session.forget("org.gnome.Nautilus");
    session.resync();
    expect(connection.sent).toEqual([
      "enable org.gnome.Nautilus",
      "disable org.gnome.Nautilus",
      "start org.gnome.Nautilus",
      "stop org.gnome.Nautilus",
      "forget org.gnome.Nautilus",
      "resync",
    ]);
  });

  /** A scrolling list reveals rows a few at a time. Coalescing one render turn
   *  makes its rows one native search batch; the dedup is what keeps a redraw
   *  from being a request at all. */
  it("coalesces icon requests, asks once per id, and batches what it sends", async () => {
    vi.useFakeTimers();
    try {
      const connection = fakeConnection();
      const session = await openSession(connection);
      session.requestIcons(["a", "b"]);
      session.requestIcons(["b", "c"]);
      expect(
        connection.sent,
        "nothing goes out before the window closes",
      ).toEqual([]);
      vi.advanceTimersByTime(0);
      expect(connection.sent).toEqual(["icons a\nb\nc"]);

      // Over one batch: split, because the extension refuses a longer request.
      connection.sent.length = 0;
      session.requestIcons(Array.from({ length: 49 }, (_, at) => `app${at}`));
      vi.advanceTimersByTime(200);
      expect(connection.sent).toHaveLength(2);
      expect(connection.sent[0]?.split("\n")).toHaveLength(48);
      expect(connection.sent[1]).toBe("icons app48");

      // Steam names hundreds of its entries "3DMark Demo.desktop", so a space
      // in an id is ordinary and must survive the batching.
      connection.sent.length = 0;
      session.requestIcons(["3DMark Demo", ""]);
      vi.advanceTimersByTime(200);
      expect(connection.sent).toEqual(["icons 3DMark Demo"]);

      // A panel closed inside the window must not send after it.
      connection.sent.length = 0;
      session.requestIcons(["late"]);
      session.close();
      vi.advanceTimersByTime(200);
      expect(connection.sent).toEqual([]);
    } finally {
      vi.useRealTimers();
    }
  });

  /** The supervisor bounds what it will queue for one panel, so an answer can
   *  be lost. Without an expiry the id stays marked asked and that row keeps
   *  its placeholder for the life of the channel. */
  it("asks again for an id whose answer never came, but not for one answered", async () => {
    vi.useFakeTimers();
    try {
      const connection = fakeConnection();
      const session = await openSession(connection);
      session.requestIcons(["lost", "found"]);
      vi.advanceTimersByTime(200);
      expect(connection.sent).toEqual(["icons lost\nfound"]);

      // One of the two is answered; the other never is.
      connection.inbound.deliver?.(
        encode({ type: "icon", id: "found", path: "/i/found.png" }),
      );

      // Still inside the window: neither is asked again.
      connection.sent.length = 0;
      session.requestIcons(["lost", "found"]);
      vi.advanceTimersByTime(200);
      expect(connection.sent).toEqual([]);

      // Past it: only the one still without an answer.
      vi.advanceTimersByTime(9000);
      session.requestIcons(["lost", "found"]);
      vi.advanceTimersByTime(200);
      expect(connection.sent).toEqual(["icons lost"]);
    } finally {
      vi.useRealTimers();
    }
  });

  it("stops sending once closed", async () => {
    const connection = fakeConnection();
    const session = await openSession(connection);
    session.close();
    session.start("a");
    expect(connection.sent).toEqual([]);
  });

  it("bounds queued peer ids and reconciles them against fresh state", async () => {
    vi.useFakeTimers();
    try {
      const connection = fakeConnection();
      const session = await openSession(connection);
      const ids = Array.from(
        { length: SESSION_ICON_QUEUE_MAX_ENTRIES + 100 },
        (_, index) => `app-${index}`,
      );
      connection.inbound.deliver?.(
        state(
          [],
          ids.map((id) => ({ id, name: id })),
        ),
      );
      session.requestIcons(ids);
      vi.advanceTimersByTime(200);
      expect(
        connection.sent.flatMap((line) =>
          line.slice("icons ".length).split("\n"),
        ),
      ).toHaveLength(SESSION_ICON_QUEUE_MAX_ENTRIES);

      connection.sent.length = 0;
      session.requestIcons([ids[0] ?? "", ids.at(-1) ?? ""]);
      connection.inbound.deliver?.(state([], [{ id: ids[0], name: ids[0] }]));
      vi.advanceTimersByTime(200);
      // The first id was already asked; the removed tail id is pruned instead
      // of surviving in either the queue or retry map.
      expect(connection.sent).toEqual([]);

      connection.inbound.deliver?.(
        state([], [{ id: ids.at(-1), name: ids.at(-1) }]),
      );
      session.requestIcons([ids.at(-1) ?? ""]);
      vi.advanceTimersByTime(200);
      expect(connection.sent).toEqual([`icons ${ids.at(-1)}`]);
      session.close();
    } finally {
      vi.useRealTimers();
    }
  });

  it("keeps the full authoritative catalog in the asked-id window", async () => {
    vi.useFakeTimers();
    try {
      const connection = fakeConnection();
      const session = await openSession(connection);
      const ids = Array.from(
        { length: SESSION_ICON_REQUEST_MAX_ENTRIES + 100 },
        (_, index) => `rotated-${index}`,
      );
      connection.inbound.deliver?.(
        state(
          [],
          ids.map((id) => ({ id, name: id })),
        ),
      );
      for (let at = 0; at < ids.length; at += SESSION_ICON_QUEUE_MAX_ENTRIES) {
        session.requestIcons(
          ids.slice(at, at + SESSION_ICON_QUEUE_MAX_ENTRIES),
        );
        vi.advanceTimersByTime(200);
      }

      connection.sent.length = 0;
      session.requestIcons([ids[0] ?? ""]);
      vi.advanceTimersByTime(200);
      expect(connection.sent).toEqual([]);
      session.close();
    } finally {
      vi.useRealTimers();
    }
  });

  it("applies the queued byte cap to maximum-length peer ids", async () => {
    vi.useFakeTimers();
    try {
      const connection = fakeConnection();
      const session = await openSession(connection);
      const ids = Array.from(
        { length: SESSION_ICON_QUEUE_MAX_ENTRIES },
        (_, index) =>
          `${index}-`.padEnd(SESSION_MAX_ID_CHARS, String(index % 10)),
      );
      connection.inbound.deliver?.(
        state(
          [],
          ids.map((id) => ({ id, name: id })),
        ),
      );
      session.requestIcons(ids);
      vi.advanceTimersByTime(200);
      const sent = connection.sent.flatMap((line) =>
        line.slice("icons ".length).split("\n"),
      );
      expect(sent.length).toBeLessThan(SESSION_ICON_QUEUE_MAX_ENTRIES);
      expect(
        sent.reduce((bytes, id) => bytes + 32 + id.length * 2, 0),
      ).toBeLessThanOrEqual(SESSION_ICON_QUEUE_MAX_BYTES);
      session.close();
    } finally {
      vi.useRealTimers();
    }
  });

  it("reads artwork on the next task when an icon path arrives", async () => {
    vi.useFakeTimers();
    const create = vi.spyOn(URL, "createObjectURL").mockReturnValue("blob:now");
    try {
      const connection = fakeConnection() as ReturnType<
        typeof fakeConnection
      > & {
        readFiles: NonNullable<Parameters<typeof openSession>[0]["readFiles"]>;
      };
      const readFiles = vi.fn(
        async (
          _groups: readonly (readonly string[])[],
          _options?: { flags?: number; maxBytes?: number },
        ) => [
          {
            status: 0,
            path: "/i/now.png",
            content: new Uint8Array([1, 2, 3]),
          },
        ],
      );
      connection.readFiles = readFiles;
      const session = await openSession(connection);
      connection.inbound.deliver?.(state([], [{ id: "now", name: "Now" }]));
      connection.inbound.deliver?.(
        encode({ type: "icon", id: "now", path: "/i/now.png" }),
      );

      expect(readFiles).not.toHaveBeenCalled();
      await vi.advanceTimersByTimeAsync(0);
      expect(readFiles).toHaveBeenCalledTimes(1);
      expect(readFiles.mock.calls[0]?.[1]).toEqual({
        maxBytes: SESSION_ARTWORK_READ_CREDIT,
      });
      await Promise.resolve();
      expect(session.icon("now")).toBe("blob:now");
      session.close();
    } finally {
      create.mockRestore();
      vi.useRealTimers();
    }
  });

  it("uses one bounded receive window for a full visible icon shelf", async () => {
    vi.useFakeTimers();
    const create = vi
      .spyOn(URL, "createObjectURL")
      .mockImplementation(() => "blob:icon");
    try {
      const connection = fakeConnection() as ReturnType<
        typeof fakeConnection
      > & {
        readFiles: NonNullable<Parameters<typeof openSession>[0]["readFiles"]>;
      };
      const readFiles = vi.fn(
        async (
          groups: readonly (readonly string[])[],
          _options?: { flags?: number; maxBytes?: number },
        ) =>
          (groups[0] ?? []).map((path) => ({
            status: 0,
            path,
            content: new Uint8Array([1, 2, 3]),
          })),
      );
      connection.readFiles = readFiles;
      const session = await openSession(connection);
      const ids = Array.from({ length: 48 }, (_, index) => `app-${index}`);
      connection.inbound.deliver?.(
        state(
          [],
          ids.map((id) => ({ id, name: id })),
        ),
      );
      for (const id of ids) {
        connection.inbound.deliver?.(
          encode({ type: "icon", id, path: `/i/${id}.png` }),
        );
      }
      await vi.advanceTimersByTimeAsync(0);

      expect(readFiles).toHaveBeenCalledTimes(1);
      expect(readFiles.mock.calls[0]?.[0]?.[0]).toHaveLength(48);
      for (const call of readFiles.mock.calls) {
        expect(call[1]).toEqual({ maxBytes: SESSION_ARTWORK_READ_CREDIT });
      }
      session.close();
    } finally {
      create.mockRestore();
      vi.useRealTimers();
    }
  });

  it("does not create a blob URL when an in-flight read finishes after close", async () => {
    vi.useFakeTimers();
    const create = vi
      .spyOn(URL, "createObjectURL")
      .mockReturnValue("blob:late");
    let resolveRead: ((records: never[]) => void) | undefined;
    try {
      const connection = fakeConnection() as ReturnType<
        typeof fakeConnection
      > & {
        readFiles: NonNullable<Parameters<typeof openSession>[0]["readFiles"]>;
      };
      connection.readFiles = () =>
        new Promise((resolve) => {
          resolveRead = resolve as (records: never[]) => void;
        });
      const session = await openSession(connection);
      connection.inbound.deliver?.(state([], [{ id: "late", name: "Late" }]));
      connection.inbound.deliver?.(
        encode({ type: "icon", id: "late", path: "/i/late.png" }),
      );
      await vi.advanceTimersByTimeAsync(200);
      session.close();
      resolveRead?.([]);
      await Promise.resolve();
      expect(create).not.toHaveBeenCalled();
    } finally {
      create.mockRestore();
      vi.useRealTimers();
    }
  });

  it("revokes loaded blob URLs and stops work when the transport closes", async () => {
    vi.useFakeTimers();
    const create = vi.spyOn(URL, "createObjectURL").mockReturnValue("blob:one");
    const revoke = vi.spyOn(URL, "revokeObjectURL");
    try {
      const connection = fakeConnection() as ReturnType<
        typeof fakeConnection
      > & {
        readFiles: NonNullable<Parameters<typeof openSession>[0]["readFiles"]>;
      };
      const readFiles = vi.fn(async () => [
        {
          status: 0,
          path: "/i/one.png",
          content: new Uint8Array([1, 2, 3]),
        },
      ]);
      connection.readFiles = readFiles;
      const session = await openSession(connection);
      connection.inbound.deliver?.(state([], [{ id: "one", name: "One" }]));
      connection.inbound.deliver?.(
        encode({ type: "icon", id: "one", path: "/i/one.png" }),
      );
      await vi.advanceTimersByTimeAsync(200);
      expect(session.icon("one")).toBe("blob:one");

      connection.inbound.close?.(0, "server stopped");
      expect(revoke).toHaveBeenCalledWith("blob:one");
      connection.inbound.deliver?.(
        encode({ type: "icon", id: "two", path: "/i/two.png" }),
      );
      await vi.advanceTimersByTimeAsync(500);
      expect(readFiles).toHaveBeenCalledTimes(1);
      expect(session.icon("two")).toBeUndefined();
    } finally {
      create.mockRestore();
      revoke.mockRestore();
      vi.useRealTimers();
    }
  });
});
