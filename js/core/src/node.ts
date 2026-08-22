/**
 * Node / Bun / Deno entry point for `@yas-run/core`.
 *
 * This subpath (`@yas-run/core/node`) exposes the local-IPC building blocks
 * needed to drive a `yas server` over a unix-domain socket from a
 * non-browser runtime. It is intentionally **not** re-exported from the
 * package root: {@link NodeUnixSocketTransport} pulls in `node:net`, and the
 * Bun/Deno variants rely on runtime globals, none of which belong in a
 * browser bundle. Browser code should import transports from
 * `@yas-run/core` / `@yas-run/core/transports` instead.
 */

export { NodeUnixSocketTransport } from "./transports/unix";
export { BunUnixSocketTransport } from "./transports/unix-bun";
export { DenoUnixSocketTransport } from "./transports/unix-deno";
export type { UnixSocketTransportOptions } from "./transports/unix-base";

export { loadYasWasm } from "./node-wasm";
