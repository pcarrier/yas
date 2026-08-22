import { defineConfig } from "vitest/config";
import solidPlugin from "vite-plugin-solid";

// Most of this suite covers non-component logic, but status-bar controls need
// a real Solid DOM owner: their contract includes surviving reactive action
// replacement between the first and second click. Keep browser resolution so
// effects use Solid's client build rather than its no-op server build.
export default defineConfig({
  plugins: [solidPlugin()],
  resolve: { conditions: ["browser", "development"] },
  test: {
    environment: "jsdom",
    globals: true,
    include: [
      "src/**/__tests__/**/*.test.ts",
      "src/**/__tests__/**/*.test.tsx",
    ],
    coverage: {
      provider: "v8",
      include: ["src/sw/**/*.ts"],
      exclude: ["src/**/__tests__/**"],
      reporter: ["text", "html"],
      reportsDirectory: "coverage",
    },
  },
});
