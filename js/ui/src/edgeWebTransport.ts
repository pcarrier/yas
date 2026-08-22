export interface EdgeWebTransportConfig {
  url: string;
  certificateHash?: string;
}

function edgeWebTransportUrl(port: number): string {
  const hostname =
    (
      import.meta.env.VITE_YAS_WEBTRANSPORT_HOST as string | undefined
    )?.trim() || location.hostname;
  const host = hostname.includes(":") ? `[${hostname}]` : hostname;
  return `https://${host}${port === 443 ? "" : `:${port}`}/edge`;
}

export async function discoverEdgeWebTransport(
  signal?: AbortSignal,
): Promise<EdgeWebTransportConfig | null> {
  if (typeof WebTransport === "undefined") return null;
  try {
    const response = await fetch("/edge-transport.json", {
      cache: "no-store",
      signal,
    });
    if (!response.ok) return null;
    const body = (await response.json()) as {
      webTransport?: { port?: unknown; certificateHash?: unknown } | null;
    };
    const value = body.webTransport;
    if (
      !value ||
      typeof value.port !== "number" ||
      !Number.isInteger(value.port) ||
      value.port < 1 ||
      value.port > 65_535
    )
      return null;
    if (
      value.certificateHash !== undefined &&
      (typeof value.certificateHash !== "string" ||
        !/^[0-9a-f]{64}$/i.test(value.certificateHash))
    )
      return null;
    return {
      url: edgeWebTransportUrl(value.port),
      ...(value.certificateHash
        ? { certificateHash: value.certificateHash }
        : {}),
    };
  } catch {
    // Old edges and WebSocket-only deployments keep their reliable path.
    return null;
  }
}

export async function fetchEdgeCertificateHash(
  signal: AbortSignal,
): Promise<string | undefined> {
  const config = await discoverEdgeWebTransport(signal);
  if (!config) throw new Error("Edge WebTransport configuration unavailable");
  return config.certificateHash;
}
