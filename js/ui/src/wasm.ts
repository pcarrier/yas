import wasmBuffer from "virtual:yas-wasm";

let initPromise: Promise<typeof import("@yas-run/browser")> | null = null;

export function initWasm(): Promise<typeof import("@yas-run/browser")> {
  if (!initPromise) {
    initPromise = import("@yas-run/browser").then(async (mod) => {
      await mod.default({ module_or_path: wasmBuffer });
      return mod;
    });
  }
  return initPromise;
}
