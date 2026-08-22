/**
 * Cut wasm-bindgen's "fetch the module from beside me" fallback.
 *
 * The generated glue ends with
 *
 * ```js
 * if (module_or_path === undefined) {
 *   module_or_path = new URL('yas_browser_bg.wasm', import.meta.url);
 * }
 * ```
 *
 * and nothing in this repository can reach it: every entry point that starts
 * the module passes the buffer its own bundler inlined (`virtual:yas-wasm`),
 * and the service worker never starts it at all. Vite reads the `new URL(...)`
 * regardless and emits the `.wasm` as an asset, which costs twice:
 *
 * - the single-file app inlines that asset a *second* time, so the HTML the
 *   Edge embeds with `include_bytes!` carries two base64 copies of one 670 KB
 *   module;
 * - the service-worker build drops a content-hashed `.wasm` into `dist/assets`
 *   that the Edge cannot serve, because a hashed name cannot be named by
 *   `include_bytes!`.
 *
 * The `iife` bundles also warn (`EMPTY_IMPORT_META`): `import.meta` becomes
 * `{}` there, so the fallback would throw on `undefined` if it ever ran.
 *
 * `enforce: "pre"` is what makes this work — the branch has to be gone before
 * Vite's own asset scanner reads it. Throwing when the pattern is missing is
 * deliberate: a wasm-bindgen upgrade that rewrites this line would otherwise
 * silently restore the warning and the duplicated payload.
 *
 * Typed structurally rather than as a `Plugin`: this file sits above the
 * packages that use it, where `vite` is not resolvable, and every config that
 * imports it checks the shape at its own call site anyway.
 */
export function dropWasmUrlFallback() {
  const fallback =
    /module_or_path = new URL\((['"])yas_browser_bg\.wasm\1, import\.meta\.url\);/;
  return {
    name: "drop-wasm-url-fallback",
    enforce: "pre" as const,
    transform(code: string, id: string) {
      if (!id.split("?")[0].endsWith("yas_browser.js")) return;
      // Only the init glue has the branch; the node build and the test stub
      // are the same filename with different contents.
      if (!code.includes("module_or_path === undefined")) return;
      if (!fallback.test(code)) {
        throw new Error(
          "yas_browser.js: the module_or_path URL fallback moved; re-check the wasm-bindgen glue",
        );
      }
      return {
        code: code.replace(
          fallback,
          'throw new Error("yas: pass the wasm module explicitly; the bundle inlines it");',
        ),
        map: null,
      };
    },
  };
}
