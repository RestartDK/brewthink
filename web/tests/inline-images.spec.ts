import { expect, test } from "@playwright/test";
import path from "node:path";
import { readFile } from "node:fs/promises";
import { captureFrame } from "./capture-frame";

test("streams a large illustration into the text flow and reuses its pixels on return", async ({ page }, testInfo) => {
  const errors: string[] = [];
  page.on("pageerror", (error) => errors.push(error.message));
  await page.goto("/");
  await expect(page.getByText("Rust/WASM 0.1.0")).toBeVisible();
  await page.locator("#epub-file").setInputFiles(path.resolve("tests/fixtures/inline-images.epub"));
  await expect(page.locator("#file-summary")).toHaveText("inline-images.epub");
  await expect(page.locator("#display-placeholder")).toBeHidden();
  await page.locator(".device-viewport").focus();
  await page.keyboard.press("Enter");
  await expect(page.locator("#selected-title")).toHaveText("Streaming illustrations");
  await page.keyboard.press("Enter");
  await expect(page.locator("#preview-heading")).toHaveText("Book cover · 480 × 800");
  const openingPath = testInfo.outputPath("opening.png");
  await captureFrame(page, openingPath);
  const opening = await readFile(openingPath);
  await page.keyboard.press("Enter");
  await expect(page.locator("#preview-heading")).toHaveText("EPUB reader · 480 × 800");
  await expect(page.locator("#selection-position")).toHaveText("Chapter 1 / 2");
  await page.locator("#display").evaluate((element) => {
    if (!(element instanceof HTMLCanvasElement)) throw new Error("Missing canvas");
    const context = element.getContext("2d");
    if (context === null) throw new Error("Missing context");
    const { data } = context.getImageData(0, 0, 480, 800);
    let top: number | undefined;
    for (let y = 24; y < 776; y += 1) {
      if (data[(y * 480 + 168) * 4] === 85) {
        top = y - 24;
        break;
      }
    }
    if (top === undefined) throw new Error("The illustration was not rendered");
    if (top < 48 || top + 240 >= 776) throw new Error("The image did not leave room for its surrounding text");
    const tones = [0, 85, 170, 255];
    for (let y = 0; y < 240; y += 1) {
      for (let x = 0; x < 320; x += 1) {
        const expected = x >= 16 && x < 304 && y >= 24 && y < 216 ? tones[Math.floor((x - 16) / 72)] : 255;
        const index = ((top + y) * 480 + 80 + x) * 4;
        if (data[index] !== expected || data[index + 1] !== expected || data[index + 2] !== expected || data[index + 3] !== 255) {
          throw new Error(`Illustration pixel differs at ${x},${y}`);
        }
      }
    }
  });
  const firstPath = testInfo.outputPath("inline-first.png");
  await captureFrame(page, firstPath);
  const first = await readFile(firstPath);
  await page.keyboard.press("ArrowRight");
  await expect(page.locator("#selection-position")).toHaveText("Chapter 2 / 2");
  await page.keyboard.press("ArrowLeft");
  await expect(page.locator("#selection-position")).toHaveText("Chapter 1 / 2");
  const returnedPath = testInfo.outputPath("inline-return.png");
  await captureFrame(page, returnedPath);
  expect(await readFile(returnedPath)).toEqual(first);
  await page.keyboard.press("p");
  await expect(page.locator("#preview-heading")).toHaveText("Retained sleep screen · 480 × 800");
  const asleepPath = testInfo.outputPath("sleep.png");
  await captureFrame(page, asleepPath);
  expect(await readFile(asleepPath)).toEqual(opening);
  expect(errors).toEqual([]);
});
