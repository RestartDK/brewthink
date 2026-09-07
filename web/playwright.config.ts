import { defineConfig } from "@playwright/test";
import { testPort } from "./test-port";

const port = testPort("BREWTHINK_WEB_PORT", 4173);
const origin = `http://127.0.0.1:${port}`;

export default defineConfig({
  testDir: "./tests",
  testIgnore: "**/dev-server.spec.ts",
  timeout: 60_000,
  fullyParallel: false,
  workers: 1,
  forbidOnly: true,
  retries: 0,
  reporter: "line",
  use: {
    baseURL: origin,
    trace: "retain-on-failure",
  },
  webServer: {
    command: `vite preview --host 127.0.0.1 --port ${port} --strictPort`,
    url: origin,
    reuseExistingServer: false,
    timeout: 180_000,
  },
});
