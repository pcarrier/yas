import { defineConfig } from "vite";
import { dropWasmUrlFallback } from "../vite-wasm-fallback";
import { brotliCompressSync, constants as zlibConstants } from "node:zlib";
import { writeFileSync } from "node:fs";
import { resolve } from "node:path";

/**
 * Second build, for the preview service worker (docs/design/net.md § Reserve
 * the prefix server-side).
 *
 * The app build inlines everything into one HTML file with
 * `vite-plugin-singlefile`, and a service worker cannot be inlined — it needs
 * its own URL and a JavaScript MIME type. So it gets its own config: no
 * single-file plugin, one entry, an IIFE bundle at a stable name the Edge service
 * embeds beside `index.html.br`.
 */
export default defineConfig({
  resolve: {
    alias: {
      "@yas-run/browser": resolve(
        __dirname,
        "../../crates/browser/pkg/yas_browser.js",
      ),
    },
  },
  build: {
    outDir: "dist",
    // The app build runs first and its output must survive this one.
    emptyOutDir: false,
    target: "es2022",
    rollupOptions: {
      input: resolve(__dirname, "src/sw/index.ts"),
      output: {
        // A service worker cannot be an ES module in every browser that
        // otherwise supports one, and this bundle has no reason to be: it is
        // self-contained.
        format: "iife",
        entryFileNames: "sw.js",
      },
    },
    minify: "esbuild",
  },
  plugins: [
    // The worker never starts the wasm module, but it does bundle the glue:
    // the preview net path reaches @yas-run/core, which reaches the browser
    // package. Without this the dead fallback drags a content-hashed .wasm into
    // dist/assets that the Edge has no way to serve.
    dropWasmUrlFallback(),
    {
      name: "brotli-sw",
      writeBundle(_options, bundle) {
        const entry = bundle["sw.js"];
        if (!entry || entry.type !== "chunk")
          throw new Error("service-worker build did not emit sw.js");
        const source = Buffer.from(entry.code);
        writeFileSync(
          resolve(__dirname, "dist/sw.js.br"),
          brotliCompressSync(source, {
            params: {
              [zlibConstants.BROTLI_PARAM_QUALITY]: 11,
              [zlibConstants.BROTLI_PARAM_SIZE_HINT]: source.length,
            },
          }),
        );
      },
    },
  ],
});
