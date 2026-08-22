import type { SessionId } from "./types";

export interface CreateSessionOptions {
  rows: number;
  cols: number;
  tag?: string;
  /** Run this through the target server's login shell. */
  command?: string;
  /** Exec this argv directly, without a shell. */
  argv?: readonly string[];
  cwdFromSessionId?: SessionId;
  cwd?: string;
  /** Environment overrides applied by the server. */
  env?: Readonly<Record<string, string>>;
  /** Server-enforced lifetime, armed at creation. */
  deadlineMs?: number;
  /** Whether to open an initial terminal view. Defaults to true. */
  subscribe?: boolean;
}

export interface AwaitSessionExitOptions {
  /** Reject if the terminal has not exited within this many milliseconds. */
  timeoutMs?: number;
}

/** A fixed encoded size one Surface view wants, in pixels. */
export interface SurfaceTarget {
  width: number;
  height: number;
}
