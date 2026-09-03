import { defineConfig } from "vitest/config";
import solidPlugin from "vite-plugin-solid";

export default defineConfig({
  // Tests do not need HMR's virtual /@solid-refresh module, whose file URL
  // has no drive letter on Windows.
  plugins: [solidPlugin({ hot: false })],
  test: {
    environment: "jsdom",
    globals: true,
    coverage: {
      provider: "v8",
      include: ["src/**/*.ts", "src/**/*.tsx"],
      exclude: ["src/__tests__/**"],
      reporter: ["text", "html"],
      reportsDirectory: "coverage",
    },
  },
});
