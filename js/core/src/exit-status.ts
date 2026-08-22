/**
 * Process exit status used by the native YAS Terminal lifecycle:
 *
 *  - `>= 0` — normal exit; the value is the `WEXITSTATUS` exit code.
 *  - `< 0`  — terminated by a signal; the value is the negated signal number.
 *  - {@link EXIT_STATUS_UNKNOWN} — the status has not been collected yet.
 *
 * These helpers keep the conventional `128 + signal` mapping in one place.
 */

/** Sentinel exit status meaning "not yet collected" (`i32::MIN`). */
export const EXIT_STATUS_UNKNOWN = -2147483648;

/**
 * Convert a raw `exit_status` into a conventional shell exit code:
 * unknown → `1`, normal exit → the code itself, signalled → `128 + signal`.
 */
export function exitCodeFromStatus(status: number): number {
  if (status === EXIT_STATUS_UNKNOWN) return 1;
  if (status >= 0) return status;
  return 128 + -status;
}

/**
 * Human-readable rendering of an `exit_status`, matching `yas`'s CLI output:
 * `"exited"`, `"exited(<code>)"` or `"signal(<n>)"`.
 */
export function formatExitStatus(status: number): string {
  if (status === EXIT_STATUS_UNKNOWN) return "exited";
  if (status >= 0) return `exited(${status})`;
  return `signal(${-status})`;
}
