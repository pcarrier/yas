import { describe, expect, it, vi } from "vitest";
import { MockYasTransport } from "./mock-yas-transport";

describe("MockYasTransport", () => {
  it("captures complete native frames without retaining caller buffers", () => {
    const transport = new MockYasTransport();
    const frame = new Uint8Array([1, 2, 3]);
    transport.send(frame);
    frame.fill(9);
    expect(transport.sent).toEqual([new Uint8Array([1, 2, 3])]);
  });

  it("delivers owned and borrowed native frames", () => {
    const transport = new MockYasTransport();
    const listener = vi.fn();
    transport.addEventListener("message", listener);

    transport.receive(new Uint8Array([4, 5]));
    const borrowed = new Uint8Array([6, 7]);
    transport.receiveBorrowed(borrowed);

    expect(new Uint8Array(listener.mock.calls[0]![0] as ArrayBuffer)).toEqual(
      new Uint8Array([4, 5]),
    );
    expect(listener.mock.calls[1]![0]).toBe(borrowed);
  });

  it("reports reconnect, suspend, and close lifecycle", () => {
    const transport = new MockYasTransport();
    const listener = vi.fn();
    transport.addEventListener("statuschange", listener);

    transport.reconnect();
    transport.suspend();
    transport.close();

    expect(transport.reconnectCount).toBe(1);
    expect(transport.suspendCount).toBe(1);
    expect(transport.status).toBe("closed");
    expect(listener.mock.calls.map(([status]) => status)).toEqual([
      "disconnected",
      "connecting",
      "disconnected",
      "closed",
    ]);
  });

  it("removes listeners exactly", () => {
    const transport = new MockYasTransport();
    const listener = vi.fn();
    transport.addEventListener("message", listener);
    transport.removeEventListener("message", listener);
    transport.receive(new Uint8Array([8]));
    expect(listener).not.toHaveBeenCalled();
  });
});
