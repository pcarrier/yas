import {
  MPRIS_CAN_CONTROL,
  MPRIS_CAN_GO_NEXT,
  MPRIS_CAN_PAUSE,
  MPRIS_CAN_PLAY,
  MPRIS_CAN_SEEK,
  type MprisPlayer,
  type YasConnectionSnapshot,
  type YasWorkspace,
} from "@yas-run/core";
import { createSignal } from "solid-js";
import { render } from "solid-js/web";
import { afterEach, describe, expect, it, vi } from "vitest";
import { DesktopChrome } from "../DesktopChrome";
import { darkTheme, uiScale } from "../theme";

const anchor = vi.hoisted(() => ({
  engage: vi.fn(),
  release: vi.fn(),
  dispose: vi.fn(),
}));
vi.mock("../mediaSessionAnchor", () => ({
  createRemoteCommandAnchor: () => anchor,
}));

let dispose: (() => void) | undefined;
afterEach(() => {
  dispose?.();
  dispose = undefined;
  document.body.replaceChildren();
  vi.unstubAllGlobals();
  vi.clearAllMocks();
});

function mountMediaSession() {
  let metadata: MediaMetadataInit | null = null;
  let playbackState: MediaSessionPlaybackState = "none";
  const metadataWrites = vi.fn();
  const playbackWrites = vi.fn();
  const handlers = new Map<
    MediaSessionAction,
    MediaSessionActionHandler | null
  >();
  const session = {
    get metadata() {
      return metadata;
    },
    set metadata(value: MediaMetadataInit | null) {
      metadataWrites(value);
      metadata = value;
    },
    get playbackState() {
      return playbackState;
    },
    set playbackState(value: MediaSessionPlaybackState) {
      playbackWrites(value);
      playbackState = value;
    },
    setActionHandler: vi.fn(
      (action: MediaSessionAction, handler: MediaSessionActionHandler | null) =>
        handlers.set(action, handler),
    ),
    setPositionState: vi.fn(),
  };
  vi.stubGlobal("navigator", { mediaSession: session, audioSession: {} });
  vi.stubGlobal(
    "MediaMetadata",
    class {
      constructor(value: MediaMetadataInit) {
        Object.assign(this, value);
      }
    },
  );

  const player: MprisPlayer = {
    playerId: 7n,
    revision: 1n,
    trackRevision: 1n,
    active: true,
    playbackStatus: "playing",
    loopStatus: "none",
    shuffle: false,
    capabilityFlags:
      MPRIS_CAN_CONTROL |
      MPRIS_CAN_PLAY |
      MPRIS_CAN_PAUSE |
      MPRIS_CAN_GO_NEXT |
      MPRIS_CAN_SEEK,
    rate: 1,
    minimumRate: 1,
    maximumRate: 1,
    volume: 1,
    positionUs: 1_000_000,
    lengthUs: 120_000_000,
    identity: "Player",
    desktopEntry: "player",
    title: "First track",
    album: "Album",
    artists: ["Artist"],
    artwork: null,
    receivedAtMs: 0,
  };
  const store = {
    players: new Map([[player.playerId, player]]),
    subscribe: vi.fn(),
    act: vi.fn().mockResolvedValue(undefined),
    positionUs: () => 1_000_000,
  };
  const connection = {
    mprisStore: store,
    mediaStore: { requests: new Map() },
  };
  const workspace = {
    getConnection: () => connection,
  } as unknown as YasWorkspace;
  const [connections, setConnections] = createSignal([
    { id: "dev", supportsDesktopMedia: true } as YasConnectionSnapshot,
  ]);
  const refresh = () => setConnections((previous) => [...previous]);
  dispose = render(
    () => (
      <DesktopChrome
        workspace={workspace}
        connections={connections()}
        connectionLabels={new Map()}
        readOnlyConnections={new Set()}
        theme={darkTheme}
        scale={uiScale(13)}
        compact={false}
        focusedConnectionId="dev"
      />
    ),
    document.body,
  );
  return {
    session,
    metadataWrites,
    playbackWrites,
    handlers,
    connection,
    store,
    player,
    refresh,
    replace(next: MprisPlayer | null) {
      connection.mprisStore.players.clear();
      if (next) connection.mprisStore.players.set(next.playerId, next);
      refresh();
    },
  };
}

describe("desktop Media Session", () => {
  it("retains Now Playing and its controls across routine snapshots", () => {
    const fixture = mountMediaSession();
    const { session, metadataWrites, playbackWrites, refresh } = fixture;
    expect(session.metadata?.title).toBe("First track");
    expect(session.playbackState).toBe("playing");
    const mediaButton = document.querySelector('button[title="Pause"]');
    expect(mediaButton).not.toBeNull();
    metadataWrites.mockClear();
    playbackWrites.mockClear();
    session.setActionHandler.mockClear();

    for (let index = 0; index < 10; index++) refresh();

    expect(metadataWrites).not.toHaveBeenCalled();
    expect(playbackWrites).not.toHaveBeenCalled();
    expect(session.setActionHandler).not.toHaveBeenCalled();
    expect(anchor.release).not.toHaveBeenCalled();
    expect(document.querySelector('button[title="Pause"]')).toBe(mediaButton);
  });

  it("updates metadata and capabilities without clearing the session", () => {
    const fixture = mountMediaSession();
    const { session, metadataWrites, playbackWrites, player, replace } =
      fixture;
    metadataWrites.mockClear();
    playbackWrites.mockClear();
    session.setActionHandler.mockClear();

    replace({
      ...player,
      title: "Second track",
      playbackStatus: "paused",
      capabilityFlags: player.capabilityFlags & ~MPRIS_CAN_GO_NEXT,
      lengthUs: 0,
    });

    expect(metadataWrites).toHaveBeenCalledExactlyOnceWith(
      expect.objectContaining({ title: "Second track" }),
    );
    expect(playbackWrites).toHaveBeenCalledExactlyOnceWith("paused");
    expect(session.setActionHandler).toHaveBeenCalledExactlyOnceWith(
      "nexttrack",
      null,
    );
    expect(session.setPositionState).toHaveBeenLastCalledWith();
  });

  it("routes retained handlers through the current store and track", () => {
    const fixture = mountMediaSession();
    const { handlers, connection, store, player, replace } = fixture;
    const seek = handlers.get("seekto")!;
    const nextStore = { ...store, players: new Map(), act: vi.fn() };
    nextStore.act.mockResolvedValue(undefined);
    connection.mprisStore = nextStore;
    replace({ ...player, playerId: 9n, trackRevision: 2n });

    expect(handlers.get("seekto")).toBe(seek);
    seek({ action: "seekto", seekTime: 12 });
    expect(store.act).not.toHaveBeenCalled();
    expect(nextStore.act).toHaveBeenCalledExactlyOnceWith(9n, {
      kind: "setPosition",
      positionUs: 12_000_000,
      trackRevision: 2n,
    });
  });

  it.each(["player removal", "unmount"])("clears on %s", (reason) => {
    const { session, metadataWrites, playbackWrites, handlers, replace } =
      mountMediaSession();
    metadataWrites.mockClear();
    playbackWrites.mockClear();
    if (reason === "player removal") replace(null);
    else {
      dispose?.();
      dispose = undefined;
    }
    expect(metadataWrites).toHaveBeenCalledExactlyOnceWith(null);
    expect(playbackWrites).toHaveBeenCalledExactlyOnceWith("none");
    expect(session.setPositionState).toHaveBeenLastCalledWith();
    expect([...handlers.values()].every((handler) => handler === null)).toBe(
      true,
    );
    if (reason === "player removal") expect(anchor.release).toHaveBeenCalled();
    else expect(anchor.dispose).toHaveBeenCalled();
  });
});
