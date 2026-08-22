import wasmBuffer from "virtual:yas-wasm";
import init, * as wasm from "@yas-run/browser";

let initPromise: Promise<typeof wasm> | undefined;

export function initWasm(): Promise<typeof wasm> {
  return (initPromise ??= init({ module_or_path: wasmBuffer }).then(
    () => wasm,
  ));
}
