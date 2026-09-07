import { defineConfig } from "@playwright/test";

const port = Number(process.env.BREWTHINK_WEB_PORT ?? "4173");
if (!Number.isInteger(port) || port < 1 || port > 65535) {
  throw new Error("BREWTHINK_WEB_PORT must be a TCP port");
}
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
