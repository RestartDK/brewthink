import { defineConfig } from "@playwright/test";
import { testPort } from "./test-port";

const port = testPort("BREWTHINK_PARITY_PORT", 4185);

export default defineConfig({
  testDir: "./parity-tests",
  fullyParallel: false,
  workers: 1,
  timeout: 60_000,
  forbidOnly: true,
  retries: 0,
  reporter: "line",
  use: {
    baseURL: `http://127.0.0.1:${port}`,
    trace: "retain-on-failure",
  },
  webServer: {
    command: `vite preview --host 127.0.0.1 --port ${port} --strictPort`,
    url: `http://127.0.0.1:${port}`,
    reuseExistingServer: false,
    timeout: 60_000,
  },
});
