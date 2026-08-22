import { readFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";

import init from "@yas-run/browser";

import type { YasWasmModule } from "./TerminalStore";

/**
 * Initialise the `@yas-run/browser` WASM module in a non-browser runtime
 * (Node / Bun / Deno) and return the module namespace, ready to hand to
 * `new YasWorkspace({ wasm })`.
 *
 * Why this exists: the `@yas-run/browser` package published today is a
 * wasm-bindgen `--target web` build, so its default `init()` assumes a
 * browser that can `fetch(new URL("yas_browser_bg.wasm", import.meta.url))`.
 * Under Node/Bun there is no such fetch and `init()` rejects with an opaque,
 * stackless error. `loadYasWasm()` instead resolves the `.wasm` that ships
 * alongside `@yas-run/browser`, reads its bytes from disk and feeds them to
 * `init({ module_or_path })` — so consumers never touch raw wasm bytes and a
 * missing/incorrect asset fails with a real filesystem error.
 *
 * It is also forward-compatible with a self-initializing build (e.g. a
 * `--target nodejs` artifact resolved via the `node` export condition): such a
 * build has no `init` default export and instantiates itself on import, so we
 * detect that and return it as-is without any filesystem access.
 *
 * @param wasmPath Optional override for the `.wasm` location. Accepts a
 *   filesystem path or a `file:` URL string; defaults to the asset colocated
 *   with `@yas-run/browser`.
 */
export async function loadYasWasm(wasmPath?: string): Promise<YasWasmModule> {
  const mod = (await import("@yas-run/browser")) as unknown as YasWasmModule & {
    default?: unknown;
  };

  // A self-initializing build (`--target nodejs`/`bundler`) has already
  // instantiated the module on import and exposes no `init` default export.
  if (typeof mod.default !== "function") {
    return mod;
  }

  const location =
    wasmPath ?? import.meta.resolve("@yas-run/browser/yas_browser_bg.wasm");
  const path = location.startsWith("file:")
    ? fileURLToPath(location)
    : location;
  const bytes = await readFile(path);
  await init({
    module_or_path: bytes as unknown as Parameters<typeof init>[0],
  });
  return mod;
}
