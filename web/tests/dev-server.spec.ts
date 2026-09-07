import { expect, test } from "@playwright/test";
import { stat, utimes } from "node:fs/promises";
import path from "node:path";

test("rebuilds WASM and reloads after a Rust change", async ({ page }) => {
  const consoleErrors: string[] = [];
  page.on("console", (entry) => {
    if (entry.type() === "error") {
      consoleErrors.push(entry.text());
    }
  });

  await page.goto("/");
  await expect(page.getByText("Rust/WASM 0.1.0")).toBeVisible();
  await page.evaluate(() => sessionStorage.setItem("wasm-reload-probe", "preserved"));
  const initialTimeOrigin = await page.evaluate(() => performance.timeOrigin);
  const rustSource = path.resolve("../src/bin/web-sim.rs");
  const sourceMetadata = await stat(rustSource);
  const changedTime = new Date(Math.max(Date.now(), sourceMetadata.mtimeMs + 1_000));

  await utimes(rustSource, sourceMetadata.atime, changedTime);
  await page.waitForFunction(
    (previousTimeOrigin) => performance.timeOrigin !== previousTimeOrigin,
    initialTimeOrigin,
    { timeout: 30_000 },
  );

  await expect(page.getByText("Rust/WASM 0.1.0")).toBeVisible();
  expect(await page.evaluate(() => sessionStorage.getItem("wasm-reload-probe"))).toBe(
    "preserved",
  );
  expect(consoleErrors).toEqual([]);
});
