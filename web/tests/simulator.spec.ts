import AxeBuilder from "@axe-core/playwright";
import { expect, test } from "@playwright/test";
import { readFile, stat } from "node:fs/promises";
import path from "node:path";

const configuredImage = process.env.BREWTHINK_TEST_IMAGE;
const fixturePath = path.resolve("tests/fixtures/checker.ppm");
const imagePath = configuredImage ?? fixturePath;

test.use({ viewport: { width: 1440, height: 1000 } });

test("renders and downloads the Rust-prepared X4 frame", async ({ page }) => {
  const consoleErrors: string[] = [];
  page.on("console", (message) => {
    if (message.type() === "error") {
      consoleErrors.push(message.text());
    }
  });

  await page.goto("/");
  await expect(page.getByText("Rust/WASM 0.1.0")).toBeVisible();
  await page.locator("#source-file").setInputFiles(imagePath);

  await expect(page.locator("#frame-state")).toHaveText("Ready");
  await expect(page.locator("#payload-size")).toHaveText("46.9 KiB");
  await expect(page.locator("#display-placeholder")).toBeHidden();
  await expect(page.locator("#display")).toHaveAttribute("width", "480");
  await expect(page.locator("#display")).toHaveAttribute("height", "800");

  if (configuredImage !== undefined) {
    await expect(page.locator("#source-size")).toHaveText("720 × 720");
    await expect(page.locator("#content-size")).toHaveText("480 × 480");
    await expect(page.locator("#black-pixels")).toHaveText("101,698");
  }

  const downloadPromise = page.waitForEvent("download");
  await page.getByRole("button", { name: "Download .frame.bin" }).click();
  const download = await downloadPromise;
  const downloadPath = await download.path();
  if (downloadPath === null) {
    throw new Error("Browser did not persist the downloaded frame");
  }

  expect(download.suggestedFilename()).toMatch(/\.frame\.bin$/);
  expect((await stat(downloadPath)).size).toBe(48_000);

  const expectedFramePath = process.env.BREWTHINK_EXPECTED_FRAME;
  if (expectedFramePath !== undefined) {
    expect(await readFile(downloadPath)).toEqual(await readFile(expectedFramePath));
  }

  const accessibility = await new AxeBuilder({ page })
    .withTags(["wcag2a", "wcag2aa", "wcag21aa", "wcag22aa"])
    .analyze();
  expect(accessibility.violations).toEqual([]);

  const screenshotPath = process.env.BREWTHINK_SCREENSHOT;
  if (screenshotPath !== undefined) {
    await page.screenshot({ path: screenshotPath, fullPage: true });
  }

  expect(consoleErrors).toEqual([]);
});

test("keeps the simulator usable at a narrow viewport", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto("/");
  await expect(page.getByText("Rust/WASM 0.1.0")).toBeVisible();
  await page.locator("#source-file").setInputFiles(fixturePath);
  await expect(page.locator("#frame-state")).toHaveText("Ready");
  await expect(page.locator("#display")).toBeVisible();

  const dimensions = await page.evaluate(() => ({
    viewport: document.documentElement.clientWidth,
    content: document.documentElement.scrollWidth,
  }));
  expect(dimensions.content).toBeLessThanOrEqual(dimensions.viewport);
});
