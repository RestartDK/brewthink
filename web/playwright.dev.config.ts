import { defineConfig } from "@playwright/test";
import production from "./playwright.config";

const port = Number(process.env.BREWTHINK_WEB_PORT ?? "4174");
if (!Number.isInteger(port) || port < 1 || port > 65535) {
  throw new Error("BREWTHINK_WEB_PORT must be a TCP port");
}
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
