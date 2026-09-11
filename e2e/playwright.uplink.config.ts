import { defineConfig } from "@playwright/test";
import config from "./playwright.config";

// The uplink spec owns its complete topology, including Edge.
export default defineConfig({
  ...config,
  testMatch: "uplink.spec.ts",
  webServer: undefined,
});
