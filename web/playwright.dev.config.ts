import { defineConfig } from "@playwright/test";
import production from "./playwright.config";
import { testPort } from "./test-port";

const port = testPort("BREWTHINK_WEB_PORT", 4174);
const baseURL = `http://127.0.0.1:${port}`;

export default defineConfig({
  ...production,
  testIgnore: [],
  testMatch: "**/dev-server.spec.ts",
  use: {
    ...production.use,
    baseURL,
  },
  webServer: {
    command: `bun run wasm && vite --host 127.0.0.1 --port ${port} --strictPort`,
    url: baseURL,
    reuseExistingServer: false,
    timeout: 180_000,
  },
});
