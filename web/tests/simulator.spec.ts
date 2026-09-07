import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";
import { readFile } from "node:fs/promises";
import path from "node:path";
import { captureFrame } from "./capture-frame";

const fixturePath = path.resolve("tests/fixtures/minimal.epub");
const noCoverFixturePath = path.resolve("tests/fixtures/no-cover.epub");
const configuredEpub = process.env.BREWTHINK_TEST_EPUB;
const walkthroughDirectory = process.env.BREWTHINK_WALKTHROUGH_DIR;

test.use({ viewport: { width: 1440, height: 1000 } });

async function continueFromCover(page: Page): Promise<void> {
  await expect(page.locator("#preview-heading")).toHaveText("Book cover · 480 × 800");
  await page.keyboard.press("Enter");
}

test("serves production assets without the Vite development client", async ({ page }) => {
  await page.goto("/");
  const modules = page.locator('script[type="module"][src]');
  await expect(modules).toHaveCount(1);
  await expect(modules).toHaveAttribute("src", /^\/assets\/.+\.js$/);
});

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
  await expect(page.locator("#preview-heading")).toHaveText("Home menu · 480 × 800");
  await expect(page.locator("#selected-title")).toHaveText("Books");
  await expect(page.locator("#selected-creator")).toHaveText("Primary menu");
  await expect(page.locator("#selection-position")).toHaveText("1 / 3");
  await expect(page.locator("#display")).toHaveAttribute("width", "480");
  await expect(page.locator("#display")).toHaveAttribute("height", "800");

  await page.keyboard.press("Enter");
  await expect(page.locator("#selected-title")).toHaveText("A Study in Scarlet");
  await page.getByRole("button", { name: "Move right" }).click();
  await expect(page.locator("#selected-title")).toHaveText("Pride and Prejudice");
  await page.keyboard.press("ArrowDown");
  await expect(page.locator("#selected-title")).toHaveText("Frankenstein");
  await expect(page.locator("#selection-position")).toHaveText("4 / 4");

  await page.keyboard.press("Enter");
  await continueFromCover(page);
  await expect(page.locator("#preview-heading")).toHaveText("EPUB reader · 480 × 800");
  await expect(page.locator("#selection-position")).toHaveText("Chapter 1 / 3");
  await expect(page.locator("#view-position")).toHaveText("1 / 8");

  await page.keyboard.press("ArrowRight");
  await expect(page.locator("#view-position")).toHaveText("2 / 8");
  await page.keyboard.press("p");
  await expect(page.locator("#preview-heading")).toHaveText(
    "Retained sleep screen · 480 × 800",
  );
  await expect(page.locator("#selection-position")).toHaveText("Position retained");
  await expect(page.getByRole("button", { name: "Wake", exact: true })).toBeEnabled();

  await page.getByRole("button", { name: "Wake", exact: true }).click();
  await expect(page.locator("#preview-heading")).toHaveText("EPUB reader · 480 × 800");
  await expect(page.locator("#view-position")).toHaveText("2 / 8");
  await page.keyboard.press("Escape");
  await expect(page.locator("#preview-heading")).toHaveText("Library shelf · 480 × 800");
  await expect(page.locator("#selected-title")).toHaveText("Frankenstein");
  await page.keyboard.press("Escape");
  await expect(page.locator("#preview-heading")).toHaveText("Home menu · 480 × 800");

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
  await expect(page.locator("#preview-heading")).toHaveText("Home menu · 480 × 800");
  await page.keyboard.press("Enter");
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
  await continueFromCover(page);
  await expect(page.locator("#preview-heading")).toHaveText("EPUB reader · 480 × 800");
  await expect(page.locator("#selection-position")).toContainText("Chapter 1 /");
  await expect(page.locator("#message")).toContainText("saved progress");

  const screenshotPath = process.env.BREWTHINK_SCREENSHOT;
  if (screenshotPath !== undefined) {
    await captureFrame(page, screenshotPath);
  }
});

test("uses a shelf placeholder when a valid EPUB has no cover", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByText("Rust/WASM 0.1.0")).toBeVisible();
  await page.locator("#epub-file").setInputFiles(noCoverFixturePath);

  await expect(page.locator("#display-placeholder")).toBeHidden();
  await page.keyboard.press("Enter");
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
  await expect(page.locator("#selected-title")).toHaveText("Books");
  await expect(page.locator("#display-placeholder")).toBeHidden();
});

test("opens files and applies reader typography settings", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByText("Rust/WASM 0.1.0")).toBeVisible();

  await page.keyboard.press("ArrowDown");
  await page.keyboard.press("Enter");
  await expect(page.locator("#preview-heading")).toHaveText("File browser · 480 × 800");
  await expect(page.locator("#selected-title")).toHaveText("study-in-scarlet.epub");
  await page.keyboard.press("Escape");

  await page.keyboard.press("ArrowDown");
  await page.keyboard.press("Enter");
  await expect(page.locator("#preview-heading")).toHaveText("Reader settings · 480 × 800");
  await expect(page.locator("#view-position")).toHaveText("Noto Serif");
  await page.keyboard.press("ArrowRight");
  await expect(page.locator("#view-position")).toHaveText("Compact");
  await page.keyboard.press("ArrowDown");
  await page.keyboard.press("ArrowRight");
  await expect(page.locator("#view-position")).toHaveText("Large");
  await page.keyboard.press("ArrowDown");
  await page.keyboard.press("ArrowRight");
  await expect(page.locator("#view-position")).toHaveText("Relaxed");
  await page.keyboard.press("ArrowDown");
  await expect(page.locator("#view-position")).toHaveText("Automatic");
  await page.keyboard.press("ArrowRight");
  await expect(page.locator("#view-position")).toHaveText("Custom image");
  await page.keyboard.press("ArrowDown");
  await page.keyboard.press("Enter");
  await expect(page.locator("#preview-heading")).toHaveText("Home menu · 480 × 800");
  await page.keyboard.press("Enter");
  await expect(page.locator("#view-position")).toHaveText("Compact");
  await page.keyboard.press("Escape");
  await page.keyboard.press("ArrowUp");
  await page.keyboard.press("ArrowUp");
  await page.keyboard.press("Enter");
  await page.keyboard.press("Enter");
  await continueFromCover(page);
  await expect(page.locator("#preview-heading")).toHaveText("EPUB reader · 480 × 800");
  await expect(page.locator("#view-position")).toHaveText("1 / 2");

  await page.reload();
  await expect(page.locator("#preview-heading")).toHaveText("Home menu · 480 × 800");
  await page.keyboard.press("ArrowDown");
  await page.keyboard.press("ArrowDown");
  await page.keyboard.press("Enter");
  await expect(page.locator("#view-position")).toHaveText("Compact");
});

test("opens and selects images from Files", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByText("Rust/WASM 0.1.0")).toBeVisible();

  await page.keyboard.press("ArrowDown");
  await page.keyboard.press("Enter");
  for (let index = 0; index < 5; index += 1) {
    await page.keyboard.press("ArrowDown");
  }
  await expect(page.locator("#selected-title")).toHaveText("AYA.JPG");
  await page.keyboard.press("Enter");
  await expect(page.locator("#preview-heading")).toHaveText("Image viewer · 480 × 800");
  await expect(page.locator("#selected-creator")).toHaveText("Confirm to select for sleep");
  await page.keyboard.press("Enter");
  await expect(page.locator("#selected-creator")).toHaveText("Selected for sleep");

  await page.reload();
  await expect(page.locator("#frame-payload")).toHaveText("96,000 bytes · 4 tones");
  await page.keyboard.press("p");
  await expect(page.locator("#selected-title")).toHaveText("AYA.JPG");
});

test("preserves every grayscale tone in images, covers, and sleep", async ({ page }) => {
  const tones = 4;
  const payload = "96,000 bytes · 4 tones";
  const palette = async (): Promise<number[]> => page.locator("#display").evaluate((element) => {
    if (!(element instanceof HTMLCanvasElement)) throw new Error("Missing canvas");
    const context = element.getContext("2d");
    if (context === null) throw new Error("Missing canvas context");
    const pixels = context.getImageData(0, 0, 480, 800).data;
    const shades = new Set<number>();
    for (let index = 0; index < pixels.length; index += 4) {
      const red = pixels[index];
      if (red === undefined) throw new Error("Truncated canvas pixel");
      shades.add(red);
    }
    return [...shades].sort((left, right) => left - right);
  });
  const capture = async (name: string): Promise<void> => {
    if (walkthroughDirectory === undefined) return;
    await captureFrame(page, path.join(walkthroughDirectory, `${name}.png`));
  };
  await page.goto("/");
  await expect(page.locator("#frame-payload")).toHaveText(payload);
  await page.keyboard.press("ArrowDown");
  await page.keyboard.press("Enter");
  for (let index = 0; index < 5; index += 1) await page.keyboard.press("ArrowDown");
  await page.keyboard.press("Enter");
  await expect(page.locator("#preview-heading")).toHaveText("Image viewer · 480 × 800");
  expect(await palette()).toEqual([0, 85, 170, 255]);
  await page.keyboard.press("Enter");
  await page.keyboard.press("p");
  await expect(page.locator("#preview-heading")).toHaveText("Retained sleep screen · 480 × 800");
  expect(await palette()).toEqual([0, 85, 170, 255]);
  await capture(`grayscale-image-${tones}`);
  await page.keyboard.press("p");
  await page.locator("#epub-file").setInputFiles(path.resolve("tests/fixtures/gray-cover.epub"));
  await expect(page.locator("#file-summary")).toHaveText("gray-cover.epub");
  await expect(page.locator("#preview-heading")).toHaveText("Home menu · 480 × 800");
  await page.keyboard.press("Enter");
  await expect(page.locator("#preview-heading")).toHaveText("Library shelf · 480 × 800");
  expect(await palette()).toEqual([0, 85, 170, 255]);
  await capture(`grayscale-cover-${tones}`);
  await page.keyboard.press("Enter");
  await page.keyboard.press("p");
  await expect(page.locator("#preview-heading")).toHaveText("Retained sleep screen · 480 × 800");
  expect(await palette()).toEqual([0, 85, 170, 255]);
});

test("uses custom sleep away from reading and a cover inside the reader", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByText("Rust/WASM 0.1.0")).toBeVisible();

  await page.keyboard.press("p");
  await expect(page.locator("#preview-heading")).toHaveText(
    "Retained sleep screen · 480 × 800",
  );
  await expect(page.locator("#selected-title")).toHaveText("ANOTHE.JPG");
  await page.keyboard.press("p");

  await page.keyboard.press("Enter");
  await page.keyboard.press("Enter");
  await continueFromCover(page);
  await expect(page.locator("#preview-heading")).toHaveText("EPUB reader · 480 × 800");
  await page.keyboard.press("p");
  await expect(page.locator("#preview-heading")).toHaveText(
    "Retained sleep screen · 480 × 800",
  );
  await expect(page.locator("#selected-title")).toHaveText("A Study in Scarlet");
});

test("keeps the reader simulator usable at a narrow viewport", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto("/");
  await expect(page.getByText("Rust/WASM 0.1.0")).toBeVisible();
  await expect(page.locator("#selected-title")).toHaveText("Books");
  await page.keyboard.press("Enter");
  await page.keyboard.press("Enter");
  await continueFromCover(page);
  await expect(page.locator("#preview-heading")).toHaveText("EPUB reader · 480 × 800");
  await expect(page.locator("#display")).toBeVisible();

  const dimensions = await page.evaluate(() => ({
    viewport: document.documentElement.clientWidth,
    content: document.documentElement.scrollWidth,
  }));
  expect(dimensions.content).toBeLessThanOrEqual(dimensions.viewport);
});

test("uses one muted gray focus outline across browser controls", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByText("Rust/WASM 0.1.0")).toBeVisible();
  const focusColor = "oklch(0.42 0.008 78)";

  await page.keyboard.press("Tab");
  for (const selector of [
    ".skip-link",
    ".device-viewport",
    "#save-frame",
    ".front-key:not(:disabled)",
    ".side-key:not(:disabled)",
    "#power-button",
  ]) {
    const control = page.locator(selector).first();
    await control.focus();
    const outline = await control.evaluate((element) => {
      const style = getComputedStyle(element);
      return {
        color: style.outlineColor,
        style: style.outlineStyle,
        width: style.outlineWidth,
      };
    });
    expect(outline).toEqual({ color: focusColor, style: "solid", width: "3px" });
  }

  await page.locator("#epub-file").focus();
  const dropOutline = await page.locator(".drop-zone").evaluate((element) => {
    const style = getComputedStyle(element);
    return {
      color: style.outlineColor,
      style: style.outlineStyle,
      width: style.outlineWidth,
    };
  });
  expect(dropOutline).toEqual({ color: focusColor, style: "solid", width: "3px" });
});

test("captures the app-shell visual walkthrough", async ({ page }) => {
  test.skip(walkthroughDirectory === undefined, "BREWTHINK_WALKTHROUGH_DIR is not set");
  if (walkthroughDirectory === undefined) {
    return;
  }
  await page.goto("/");
  await expect(page.getByText("Rust/WASM 0.1.0")).toBeVisible();
  await page.screenshot({
    path: path.join(walkthroughDirectory, "00-simulator.png"),
    fullPage: true,
  });
  await captureFrame(page, path.join(walkthroughDirectory, "01-home.png"));

  await page.keyboard.press("Enter");
  await expect(page.locator("#preview-heading")).toHaveText("Library shelf · 480 × 800");
  await captureFrame(page, path.join(walkthroughDirectory, "02-books.png"));

  await page.keyboard.press("Escape");
  await page.keyboard.press("ArrowDown");
  await page.keyboard.press("Enter");
  await expect(page.locator("#preview-heading")).toHaveText("File browser · 480 × 800");
  await captureFrame(page, path.join(walkthroughDirectory, "03-files.png"));

  await page.keyboard.press("Escape");
  await page.keyboard.press("ArrowDown");
  await page.keyboard.press("Enter");
  await expect(page.locator("#view-position")).toHaveText("Noto Serif");
  await captureFrame(page, path.join(walkthroughDirectory, "04-settings.png"));

  await page.keyboard.press("ArrowDown");
  await page.keyboard.press("ArrowDown");
  await page.keyboard.press("ArrowDown");
  await page.keyboard.press("ArrowDown");
  await page.keyboard.press("Enter");
  await page.keyboard.press("ArrowUp");
  await page.keyboard.press("ArrowUp");
  await page.keyboard.press("Enter");
  await page.keyboard.press("Enter");
  await expect(page.locator("#preview-heading")).toHaveText("Book cover · 480 × 800");
  await captureFrame(page, path.join(walkthroughDirectory, "05-cover.png"));
  await continueFromCover(page);
  await expect(page.locator("#preview-heading")).toHaveText("EPUB reader · 480 × 800");
  await captureFrame(page, path.join(walkthroughDirectory, "06-reader.png"));
  await page.keyboard.press("Enter");
  await expect(page.locator("#preview-heading")).toHaveText("Reading controls · 480 × 800");
  await captureFrame(page, path.join(walkthroughDirectory, "07-drawer.png"));
  await page.keyboard.press("Escape");

  await page.keyboard.press("p");
  await expect(page.locator("#preview-heading")).toHaveText(
    "Retained sleep screen · 480 × 800",
  );
  await captureFrame(page, path.join(walkthroughDirectory, "08-sleep.png"));
});

test("opens the drawer, stages page and chapter jumps, and reflows typography", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByText("Rust/WASM 0.1.0")).toBeVisible();
  await page.keyboard.press("Enter");
  await page.keyboard.press("Enter");
  await continueFromCover(page);
  const original = await page.locator("#view-position").textContent();
  await page.keyboard.press("Enter");
  await expect(page.locator("#preview-heading")).toHaveText("Reading controls · 480 × 800");
  await expect(page.locator("#view-position")).toHaveText(original ?? "");
  await page.keyboard.press("ArrowRight");
  await expect(page.locator("#view-position")).toHaveText(original ?? "");
  await page.keyboard.press("Escape");
  await expect(page.locator("#preview-heading")).toHaveText("EPUB reader · 480 × 800");
  await expect(page.locator("#view-position")).toHaveText(original ?? "");
  await page.keyboard.press("Enter");
  await page.keyboard.press("ArrowRight");
  await page.keyboard.press("Enter");
  await expect(page.locator("#view-position")).toHaveText("2 / 8");
  await page.keyboard.press("Enter");
  await page.keyboard.press("ArrowDown");
  await expect(page.locator("#selected-creator")).toHaveText("Chapter: Section 1");
  await page.keyboard.press("ArrowRight");
  await page.keyboard.press("Enter");
  await expect(page.locator("#selection-position")).toHaveText("Chapter 2 / 3");
  await expect(page.locator("#view-position")).toHaveText("1 / 8");
  await page.keyboard.press("Enter");
  for (let row = 0; row < 3; row += 1) await page.keyboard.press("ArrowDown");
  await expect(page.locator("#selected-creator")).toHaveText("Text size");
  await page.keyboard.press("ArrowRight");
  await page.keyboard.press("Enter");
  await expect(page.locator("#preview-heading")).toHaveText("EPUB reader · 480 × 800");
  await expect(page.locator("#view-position")).not.toHaveText("1 / 8");
  const applied = await page.evaluate(() => localStorage.getItem("brewthink.reader-preferences.v1"));
  await page.reload();
  expect(await page.evaluate(() => localStorage.getItem("brewthink.reader-preferences.v1"))).toBe(applied);
});

test("exports exact native pixels independently of viewport and browser scaling", async ({ page }, testInfo) => {
  for (const width of [390, 1440]) {
    await page.setViewportSize({ width, height: 1000 });
    await page.goto("/");
    await expect(page.getByText("Rust/WASM 0.1.0")).toBeVisible();
    const size = await page.locator("#display").evaluate((element) => {
      const bounds = element.getBoundingClientRect();
      return { width: bounds.width, height: bounds.height };
    });
    expect(size).toEqual({ width: 480, height: 800 });
    const expectedPath = testInfo.outputPath(`native-${width}.png`);
    await captureFrame(page, expectedPath);
    const downloadReady = page.waitForEvent("download");
    await page.getByRole("button", { name: "Save frame PNG" }).click();
    const download = await downloadReady;
    expect(download.suggestedFilename()).toBe("brewthink-home-480x800.png");
    const savedPath = testInfo.outputPath(`download-${width}.png`);
    await download.saveAs(savedPath);
    const saved = await readFile(savedPath);
    expect(saved.equals(await readFile(expectedPath))).toBe(true);
    expect(saved.readUInt32BE(16)).toBe(480);
    expect(saved.readUInt32BE(20)).toBe(800);
  }
});
