import { defineConfig } from "@playwright/test";

const port = Number(process.env.BREWTHINK_WEB_PORT ?? "4173");
if (!Number.isInteger(port) || port < 1 || port > 65535) {
  throw new Error("BREWTHINK_WEB_PORT must be a TCP port");
}
const origin = `http://127.0.0.1:${port}`;

export default defineConfig({
  testDir: "./tests",
  fullyParallel: false,
  forbidOnly: true,
  retries: 0,
  reporter: "line",
  use: {
    baseURL: origin,
    trace: "retain-on-failure",
  },
  webServer: {
    command: `bun run wasm && vite --host 127.0.0.1 --port ${port} --strictPort`,
    url: origin,
    reuseExistingServer: process.env.BREWTHINK_REUSE_WEB_SERVER === "1",
    timeout: 180_000,
  },
});
