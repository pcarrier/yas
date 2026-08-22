// @vitest-environment node

import { createRoot } from "solid-js";
import { expect, it } from "vitest";
import {
  GIT_LOG_TOPO,
  YasConnection,
  YasEdgeWebSocketTransport,
  YasNativeWorkspaceConnection,
  YasWorkspace,
  yasBrowserConnectionOptions,
} from "@yas-run/core";
import { useIdeSession } from "../ide/session";

it.skipIf(!process.env.YAS_PASSPHRASE)(
  "loads the live development root",
  async () => {
    const transport = new YasEdgeWebSocketTransport(
      "ws://127.0.0.1:10001/edge",
      process.env.YAS_PASSPHRASE!,
      { reconnect: false },
    );
    const connection = new YasConnection(
      transport,
      yasBrowserConnectionOptions("live-test"),
    );
    const wasm = new Promise<never>(() => undefined);
    const native = new YasNativeWorkspaceConnection("local", connection, wasm);
    const workspace = new YasWorkspace({
      wasm,
      connections: [{ id: "local", connection: native }],
    });
    await connection.connect();

    let dispose!: () => void;
    let releaseTree: (() => void) | undefined;
    let releaseLog: (() => void) | undefined;
    const session = createRoot((rootDispose) => {
      dispose = rootDispose;
      return useIdeSession(workspace, () => ({
        key: "live local /src/yas",
        connectionId: "local",
        path: "/src/yas",
      }));
    });
    try {
      const deadline = Date.now() + 8_000;
      while (Date.now() < deadline) {
        const current = session();
        if (current && !releaseTree) {
          releaseTree = current.ensureTree();
          releaseLog = current.ensureLog();
        }
        if (current?.treePhase() === "live" && current.logLoaded()) {
          expect(current.tree()?.length).toBeGreaterThan(0);
          expect(current.commits().length).toBeGreaterThan(0);
          return;
        }
        await new Promise((resolve) => setTimeout(resolve, 50));
      }
      const current = session();
      throw new Error(
        JSON.stringify({
          root: current?.root(),
          phase: current?.treePhase(),
          rows: current?.tree()?.length,
          fsError: current?.fsError(),
          gitSettled: current?.gitSettled(),
          gitError: current?.gitError(),
          logLoaded: current?.logLoaded(),
          commits: current?.commits().length,
          topo: GIT_LOG_TOPO,
        }),
      );
    } finally {
      releaseTree?.();
      releaseLog?.();
      dispose();
      workspace.dispose();
    }
  },
  10_000,
);
