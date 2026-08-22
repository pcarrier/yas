import { defineConfig } from "vite";
import { dropWasmUrlFallback } from "../vite-wasm-fallback";
import solid from "vite-plugin-solid";
import tailwindcss from "@tailwindcss/vite";
import { lezer } from "@lezer/generator/rollup";
import { existsSync, readFileSync, readdirSync } from "node:fs";
import { join, resolve } from "node:path";

const root = import.meta.dirname;
const wasmPath = resolve(root, "../../crates/browser/pkg/yas_browser_bg.wasm");
const snippetsDir = resolve(root, "../../crates/browser/pkg/snippets");
const isBuild = process.argv.includes("build");

export default defineConfig({
  plugins: [
    solid(),
    tailwindcss(),
    lezer(),
    {
      name: "yas-wasm",
      resolveId(id) {
        if (id === "virtual:yas-wasm") return "\0virtual:yas-wasm";
      },
      load(id) {
        if (id !== "\0virtual:yas-wasm") return;
        if (!isBuild) return `export default "/@fs${wasmPath}"`;
        const base64 = readFileSync(wasmPath).toString("base64");
        return `
const bytes = Uint8Array.from(atob(${JSON.stringify(base64)}), c => c.charCodeAt(0));
export default bytes.buffer;
`;
      },
    },
    dropWasmUrlFallback(),
    {
      name: "yas-snippets",
      resolveId(id, importer) {
        const match = id.match(/\.\/snippets\/yas-browser-[^/]+\/(.*)/);
        if (!match || !importer || !existsSync(snippetsDir)) return;
        for (const directory of readdirSync(snippetsDir)) {
          const candidate = join(snippetsDir, directory, match[1]);
          if (existsSync(candidate)) return candidate;
        }
      },
    },
  ],
  resolve: {
    alias: {
      "@yas-run/browser": resolve(
        root,
        "../../crates/browser/pkg/yas_browser.js",
      ),
    },
    dedupe: ["solid-js"],
  },
  server: {
    strictPort: true,
    fs: { allow: [resolve(root, "../..")] },
  },
  build: {
    target: "es2020",
    modulePreload: { polyfill: false },
    rollupOptions: {
      input: [resolve(root, "index.html"), resolve(root, "s/index.html")],
    },
  },
});
