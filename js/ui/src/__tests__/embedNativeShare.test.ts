import { describe, expect, it } from "vitest";
import { shareTransport } from "../nativeShareTransport";

describe("embedded share transport", () => {
  it("returns the raw native YAS WebRTC stream for Workspace to own", () => {
    const transport = shareTransport(
      "wss://yas.run",
      "aaaaaaaaaaaaaaaaaaaaaaaaaa",
    );

    expect(["disconnected", "connecting"]).toContain(transport.status);
    expect("yasConnection" in transport).toBe(false);
    expect(typeof transport.send).toBe("function");
    transport.close();
  });
});
