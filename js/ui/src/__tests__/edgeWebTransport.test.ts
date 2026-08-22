import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  discoverEdgeWebTransport,
  fetchEdgeCertificateHash,
} from "../edgeWebTransport";

const fetchMock = vi.fn();

beforeEach(() => {
  fetchMock.mockReset();
  vi.stubGlobal("fetch", fetchMock);
  vi.stubGlobal("WebTransport", class {});
  vi.stubEnv("VITE_YAS_WEBTRANSPORT_HOST", "edge.test");
});

afterEach(() => {
  vi.unstubAllGlobals();
  vi.unstubAllEnvs();
});

function advertisement(certificateHash?: unknown) {
  return {
    ok: true,
    json: async () => ({ webTransport: { port: 4433, certificateHash } }),
  };
}

describe("edge WebTransport discovery", () => {
  it("fetches the latest certificate hash without caching the initial discovery", async () => {
    const oldHash = "ab".repeat(32);
    const newHash = "cd".repeat(32);
    fetchMock
      .mockResolvedValueOnce(advertisement(oldHash))
      .mockResolvedValueOnce(advertisement(newHash));
    expect(await discoverEdgeWebTransport()).toEqual({
      url: "https://edge.test:4433/edge",
      certificateHash: oldHash,
    });

    const signal = new AbortController().signal;
    expect(await fetchEdgeCertificateHash(signal)).toBe(newHash);
    expect(fetchMock).toHaveBeenCalledTimes(2);
    expect(fetchMock).toHaveBeenLastCalledWith("/edge-transport.json", {
      cache: "no-store",
      signal,
    });
  });

  it("allows public certificates without a pin", async () => {
    fetchMock.mockResolvedValue(advertisement());
    expect(
      await fetchEdgeCertificateHash(new AbortController().signal),
    ).toBeUndefined();
  });

  it.each(["http", "network", "disabled", "invalid pin"])(
    "fails the connection attempt when discovery returns %s",
    async (failure) => {
      if (failure === "http") fetchMock.mockResolvedValue({ ok: false });
      else if (failure === "network")
        fetchMock.mockRejectedValue(new Error("offline"));
      else if (failure === "disabled")
        fetchMock.mockResolvedValue({
          ok: true,
          json: async () => ({ webTransport: null }),
        });
      else fetchMock.mockResolvedValue(advertisement("invalid"));

      await expect(
        fetchEdgeCertificateHash(new AbortController().signal),
      ).rejects.toThrow("Edge WebTransport configuration unavailable");
    },
  );
});
