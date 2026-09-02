import AxeBuilder from "@axe-core/playwright";
import { expect, test } from "@playwright/test";
import { stat, utimes } from "node:fs/promises";
import path from "node:path";

const fixturePath = path.resolve("tests/fixtures/minimal.epub");
const noCoverFixturePath = path.resolve("tests/fixtures/no-cover.epub");
const configuredEpub = process.env.BREWTHINK_TEST_EPUB;

test.use({ viewport: { width: 1440, height: 1000 } });

test("runs the complete library, reader, sleep, wake, and resume loop", async ({
  page,
}) => {
  const consoleErrors: string[] = [];
  page.on("console", (entry) => {
    if (entry.type() === "error") {
      consoleErrors.push(entry.text());
    }
  });

  await page.goto("/");
  await expect(page.getByText("Rust/WASM 0.1.0")).toBeVisible();
  await expect(page.locator("#display-placeholder")).toBeHidden();
  await expect(page.locator("#selected-title")).toHaveText("A Study in Scarlet");
  await expect(page.locator("#selected-creator")).toHaveText("Arthur Conan Doyle");
  await expect(page.locator("#selection-position")).toHaveText("1 / 4");
  await expect(page.locator("#display")).toHaveAttribute("width", "480");
  await expect(page.locator("#display")).toHaveAttribute("height", "800");

  await page.getByRole("button", { name: "Move right" }).click();
  await expect(page.locator("#selected-title")).toHaveText("Pride and Prejudice");
  await page.keyboard.press("ArrowDown");
  await expect(page.locator("#selected-title")).toHaveText("Frankenstein");
  await expect(page.locator("#selection-position")).toHaveText("4 / 4");

  await page.keyboard.press("Enter");
  await expect(page.locator("#preview-heading")).toHaveText("EPUB reader · 480 × 800");
  await expect(page.locator("#selection-position")).toHaveText("Chapter 1 / 3");
  await expect(page.locator("#view-position")).toHaveText("1 / 2");

  await page.keyboard.press("ArrowRight");
  await expect(page.locator("#view-position")).toHaveText("2 / 2");
  await page.keyboard.press("p");
  await expect(page.locator("#preview-heading")).toHaveText(
    "Retained sleep screen · 480 × 800",
  );
  await expect(page.locator("#selection-position")).toHaveText("Position retained");
  await expect(page.getByRole("button", { name: "Wake", exact: true })).toBeEnabled();

  await page.getByRole("button", { name: "Wake", exact: true }).click();
  await expect(page.locator("#preview-heading")).toHaveText("EPUB reader · 480 × 800");
  await expect(page.locator("#view-position")).toHaveText("2 / 2");
  await page.keyboard.press("Escape");
  await expect(page.locator("#preview-heading")).toHaveText("Library shelf · 480 × 800");
  await expect(page.locator("#selected-title")).toHaveText("Frankenstein");

  const accessibility = await new AxeBuilder({ page })
    .withTags(["wcag2a", "wcag2aa", "wcag21aa", "wcag22aa"])
    .analyze();
  expect(accessibility.violations).toEqual([]);
  expect(consoleErrors).toEqual([]);
});

test("parses an EPUB, renders its cover, and opens its spine text", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByText("Rust/WASM 0.1.0")).toBeVisible();
  await page.locator("#epub-file").setInputFiles(configuredEpub ?? fixturePath);

  await expect(page.locator("#display-placeholder")).toBeHidden();
  if (configuredEpub === undefined) {
    await expect(page.locator("#selected-title")).toHaveText("Synthetic & Safe");
    await expect(page.locator("#selected-creator")).toHaveText("Fixture Author");
    await expect(page.locator("#file-summary")).toHaveText("minimal.epub");
  } else {
    await expect(page.locator("#selected-title")).toHaveText(
      "The Art of Doing Science and Engineering: Learning to Learn",
    );
    await expect(page.locator("#selected-creator")).toHaveText("Richard W. Hamming");
  }
  await expect(page.locator("#selection-position")).toHaveText("1 / 4");
  await expect(page.locator("#view-position")).toHaveText("1 / 1");
  await expect(page.locator("#reset-library")).toBeEnabled();

  await page.getByRole("button", { name: "Confirm" }).click();
  await expect(page.locator("#preview-heading")).toHaveText("EPUB reader · 480 × 800");
  await expect(page.locator("#selection-position")).toContainText("Chapter 1 /");
  await expect(page.locator("#message")).toContainText("semantic");

  const screenshotPath = process.env.BREWTHINK_SCREENSHOT;
  if (screenshotPath !== undefined) {
    await page.screenshot({ path: screenshotPath, fullPage: true });
  }
});

test("uses a shelf placeholder when a valid EPUB has no cover", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByText("Rust/WASM 0.1.0")).toBeVisible();
  await page.locator("#epub-file").setInputFiles(noCoverFixturePath);

  await expect(page.locator("#display-placeholder")).toBeHidden();
  await expect(page.locator("#selected-title")).toHaveText("Words Without a Cover");
  await expect(page.locator("#selected-creator")).toHaveText("Fixture Author");
  await page.keyboard.press("Enter");
  await expect(page.locator("#preview-heading")).toHaveText("EPUB reader · 480 × 800");
});

test("reports invalid EPUB input without losing the simulator", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByText("Rust/WASM 0.1.0")).toBeVisible();
  await page.locator("#epub-file").setInputFiles(path.resolve("tests/fixtures/checker.ppm"));

  await expect(page.locator("#selected-title")).toHaveText("EPUB rejected");
  await expect(page.locator("#message")).toContainText("InvalidZip");
  await expect(page.locator("#display-placeholder")).toBeVisible();
  await page.getByRole("button", { name: "Reset sample" }).click();
  await expect(page.locator("#selected-title")).toHaveText("A Study in Scarlet");
  await expect(page.locator("#display-placeholder")).toBeHidden();
});

test("keeps the reader simulator usable at a narrow viewport", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto("/");
  await expect(page.getByText("Rust/WASM 0.1.0")).toBeVisible();
  await expect(page.locator("#selected-title")).toHaveText("A Study in Scarlet");
  await page.keyboard.press("Enter");
  await expect(page.locator("#preview-heading")).toHaveText("EPUB reader · 480 × 800");
  await expect(page.locator("#display")).toBeVisible();

  const dimensions = await page.evaluate(() => ({
    viewport: document.documentElement.clientWidth,
    content: document.documentElement.scrollWidth,
  }));
  expect(dimensions.content).toBeLessThanOrEqual(dimensions.viewport);
});

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
