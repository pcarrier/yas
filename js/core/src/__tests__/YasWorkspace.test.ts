import { describe, expect, it, vi } from "vitest";
import { YasWorkspace } from "../YasWorkspace";
import type { YasWasmModule } from "../TerminalStore";
import { MockYasTransport } from "./mock-yas-transport";

class FakeTerminal {
  constructor(
    _rows: number,
    _cols: number,
    _pixelWidth: number,
    _pixelHeight: number,
  ) {}
  free() {}
}

const wasm = { Terminal: FakeTerminal } as unknown as YasWasmModule;

describe("YasWorkspace connection transport ownership", () => {
  it("closes the transport by default when removing a connection", () => {
    const workspace = new YasWorkspace({ wasm });
    const transport = new MockYasTransport();
    const close = vi.spyOn(transport, "close");
    const connection = workspace.addConnection({
      id: "local",
      transport,
      autoConnect: false,
    });
    const dispose = vi.spyOn(connection, "dispose");

    workspace.removeConnection("local");

    expect(workspace.getConnection("local")).toBeNull();
    expect(close).toHaveBeenCalledOnce();
    expect(dispose).toHaveBeenCalledOnce();
    expect(transport.status).toBe("closed");
  });

  it("disposes local connection state while preserving an externally owned transport", () => {
    const workspace = new YasWorkspace({ wasm });
    const transport = new MockYasTransport();
    const close = vi.spyOn(transport, "close");
    const connection = workspace.addConnection({
      id: "local",
      transport,
      autoConnect: false,
    });
    const dispose = vi.spyOn(connection, "dispose");

    workspace.removeConnection("local", { closeTransport: false });

    expect(workspace.getConnection("local")).toBeNull();
    expect(close).not.toHaveBeenCalled();
    expect(dispose).toHaveBeenCalledOnce();
    expect(transport.status).toBe("connected");

    // Once detached, disposing the workspace must not claim transport
    // ownership retroactively.
    workspace.dispose();
    expect(close).not.toHaveBeenCalled();
  });

  it("keys aggregated Surface diagnostics by their owning connection", () => {
    const workspace = new YasWorkspace({ wasm });
    const local = workspace.addConnection({
      id: "local",
      transport: new MockYasTransport(),
      autoConnect: false,
    });
    const remote = workspace.addConnection({
      id: "remote",
      transport: new MockYasTransport(),
      autoConnect: false,
    });
    type SurfaceStats = ReturnType<typeof local.surfaceStore.getDebugStats>;
    vi.spyOn(local.surfaceStore, "getDebugStats").mockReturnValue([
      { surfaceId: 1n } as SurfaceStats[number],
    ]);
    vi.spyOn(remote.surfaceStore, "getDebugStats").mockReturnValue([
      { surfaceId: 1n } as SurfaceStats[number],
    ]);

    const stats = workspace.getConnectionDebugStats("local", null);

    expect(
      stats?.surfaces.map(({ connectionId, surfaceId }) => [
        connectionId,
        surfaceId,
      ]),
    ).toEqual([
      ["local", 1n],
      ["remote", 1n],
    ]);
    workspace.dispose();
  });
});
