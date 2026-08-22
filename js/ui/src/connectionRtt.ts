import type { ConnectionStatus } from "@yas-run/core";

export interface ConnectionRttSample {
  readonly status: ConnectionStatus;
  readonly rttMs: number | null;
}

export interface ConnectionRttSummary {
  readonly minimum: number;
  readonly maximum: number;
  readonly multipleServers: boolean;
  readonly minimumText: string;
  readonly maximumText: string;
  readonly unit: "ms" | "s";
  readonly text: string;
}

/** Status-bar RTT summary: one number for one server, a min/max pair for a
 * multi-server workspace. Equal endpoint values remain a pair because the
 * shape communicates how many server latencies are represented. */
export function connectionRttSummary(
  connections: readonly ConnectionRttSample[],
): ConnectionRttSummary | null {
  let minimum: number | null = null;
  let maximum: number | null = null;
  for (const connection of connections) {
    if (
      connection.status !== "connected" ||
      typeof connection.rttMs !== "number"
    ) {
      continue;
    }
    minimum = Math.min(minimum ?? Infinity, connection.rttMs);
    maximum = Math.max(maximum ?? 0, connection.rttMs);
  }
  if (minimum === null || maximum === null) return null;
  const multipleServers = connections.length > 1;
  const unit = maximum >= 1_000 ? "s" : "ms";
  const format =
    unit === "s"
      ? (milliseconds: number) => (milliseconds / 1_000).toFixed(2)
      : (milliseconds: number) => Math.round(milliseconds).toString();
  const minimumText = format(minimum);
  const maximumText = format(maximum);
  return {
    minimum,
    maximum,
    multipleServers,
    minimumText,
    maximumText,
    unit,
    text: multipleServers
      ? `${minimumText}–${maximumText} ${unit}`
      : `${minimumText} ${unit}`,
  };
}
