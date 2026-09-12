import { expect, test, type Page } from "@playwright/test";
import path from "node:path";
import { captureFrame } from "./capture-frame";

async function framePng(page: Page): Promise<string> {
  return page.locator("#display").evaluate((element) => {
    if (!(element instanceof HTMLCanvasElement)) throw new Error("Expected frame canvas");
    return element.toDataURL("image/png");
  });
}

async function openFixture(page: Page, name = "navigation-cover.epub"): Promise<void> {
  await page.goto("/");
  await expect(page.getByText("Rust/WASM 0.1.0")).toBeVisible();
  await page.locator("#epub-file").setInputFiles(path.resolve("tests/fixtures", name));
  await expect(page.locator("#file-summary")).toHaveText(name);
  await page.keyboard.press("Enter");
  await page.keyboard.press("Enter");
}

test("aligns the two front rockers with the shared framebuffer hint centers", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByText("Rust/WASM 0.1.0")).toBeVisible();
  const frame = await page.locator("#display").boundingBox();
  if (frame === null) throw new Error("Missing frame");
  const buttons = page.locator(".front-key");
  await expect(buttons).toHaveCount(4);
  const physicalButtons = [
    { name: "Move left", center: 100 },
    { name: "Move right", center: 192 },
    { name: "Back", center: 300 },
    { name: "Confirm", center: 392 },
  ];
  for (const [index, { name, center }] of physicalButtons.entries()) {
    await expect(buttons.nth(index)).toHaveAccessibleName(name);
    const bounds = await page.getByRole("button", { name, exact: true }).boundingBox();
    if (bounds === null) throw new Error("Missing rocker");
    expect(bounds.x + bounds.width / 2 - frame.x).toBeCloseTo(center, 1);
    expect(bounds.y).toBeGreaterThan(frame.y + frame.height);
    expect(bounds.height).toBeGreaterThanOrEqual(24);
  }
  await expect(page.locator(".side-key")).toHaveCount(2);
});

test("shows every native cover pixel without chrome and reuses it for sleep", async ({ page }) => {
  await openFixture(page);
  await expect(page.locator("#preview-heading")).toHaveText("Book cover · 480 × 800");
  const mismatches = await page.locator("#display").evaluate((element) => {
    if (!(element instanceof HTMLCanvasElement)) throw new Error("Expected frame canvas");
    const context = element.getContext("2d");
    if (context === null) throw new Error("Missing canvas context");
    const { data } = context.getImageData(0, 0, 480, 800);
    let count = 0;
    for (let y = 0; y < 800; y += 1) {
      for (let x = 0; x < 480; x += 1) {
        const expected = x % 8 === 0 || (x < 240 && y % 8 === 0) ? 0 : 255;
        if (data[(y * 480 + x) * 4] !== expected) count += 1;
      }
    }
    return count;
  });
  expect(mismatches).toBe(0);
  const cover = await framePng(page);
  await page.keyboard.press("Escape");
  await expect(page.locator("#preview-heading")).toHaveText("Library shelf · 480 × 800");
  await page.keyboard.press("Enter");
  expect(await framePng(page)).toBe(cover);
  await page.keyboard.press("p");
  await expect(page.locator("#preview-heading")).toHaveText("Retained sleep screen · 480 × 800");
  expect(await framePng(page)).toBe(cover);
  await page.keyboard.press("p");
  await expect(page.locator("#preview-heading")).toHaveText("EPUB reader · 480 × 800");
  await page.keyboard.press("Escape");
  await page.keyboard.press("Enter");
  await expect(page.locator("#preview-heading")).toHaveText("EPUB reader · 480 × 800");
});

test("previews named chapters and moves to either end of the book", async ({ page }) => {
  await openFixture(page);
  await page.keyboard.press("Enter");
  const directory = process.env.BREWTHINK_WALKTHROUGH_DIR;
  if (directory !== undefined) await captureFrame(page, path.join(directory, "10-reader.png"));
  await page.keyboard.press("Enter");
  await page.keyboard.press("ArrowDown");
  await expect(page.locator("#selected-creator")).toHaveText("Chapter: A quiet morning");
  await page.keyboard.press("ArrowRight");
  await expect(page.locator("#selected-creator")).toHaveText("Chapter: Along the river");
  if (directory !== undefined) await captureFrame(page, path.join(directory, "09-named-chapter.png"));
  await page.keyboard.press("Enter");
  await expect(page.locator("#selection-position")).toHaveText("Chapter 2 / 3");
  await page.keyboard.press("Enter");
  for (let step = 0; step < 20; step += 1) await page.keyboard.press("ArrowRight");
  await expect(page.locator("#selected-creator")).toHaveText("Book position: 100%");
  await page.keyboard.press("Enter");
  await expect(page.locator("#selection-position")).toHaveText("Chapter 3 / 3");
  const lastPage = await page.locator("#view-position").textContent();
  const [current, count] = (lastPage ?? "").split(" / ");
  expect(current).toBe(count);
  await page.keyboard.press("Enter");
  for (let step = 0; step < 20; step += 1) await page.keyboard.press("ArrowLeft");
  await expect(page.locator("#selected-creator")).toHaveText("Book position: 0%");
  await page.keyboard.press("Enter");
  await expect(page.locator("#selection-position")).toHaveText("Chapter 1 / 3");
  await expect(page.locator("#view-position")).toHaveText(/^1 \/ /);
});

test("skips a corrupt optional cover rather than rejecting readable text", async ({ page }) => {
  await openFixture(page, "invalid-cover.epub");
  await expect(page.locator("#preview-heading")).toHaveText("EPUB reader · 480 × 800");
});
