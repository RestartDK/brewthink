import { expect, test, type Page } from "@playwright/test";
import { captureFrame } from "./capture-frame";

async function expectMonochrome(page: Page): Promise<void> {
  const intermediatePixels = await page.locator("#display").evaluate((element) => {
    if (!(element instanceof HTMLCanvasElement)) throw new Error("Missing display canvas");
    const context = element.getContext("2d");
    if (context === null) throw new Error("Missing display context");
    const { data } = context.getImageData(0, 0, 480, 800);
    let count = 0;
    for (let index = 0; index < data.length; index += 4) {
      if (data[index] !== 0 && data[index] !== 255) count += 1;
    }
    return count;
  });
  expect(intermediatePixels).toBe(0);
}

test("interactive menus use monochrome pixels, including image previews", async ({ page }, testInfo) => {
  const capture = async (name: string): Promise<void> => {
    await expectMonochrome(page);
    await captureFrame(page, testInfo.outputPath(`${name}.png`));
  };
  await page.goto("/");
  await expect(page.getByText("Rust/WASM 0.1.0")).toBeVisible();
  await capture("dither-home");
  await page.keyboard.press("Enter");
  await expect(page.locator("#preview-heading")).toHaveText("Books list · 480 × 800");
  await capture("dither-books");
  await page.keyboard.press("ArrowDown");
  await page.keyboard.press("Enter");
  await expect(page.locator("#preview-heading")).toHaveText("Book cover · 480 × 800");
  await page.keyboard.press("Enter");
  await page.keyboard.press("Enter");
  await expect(page.locator("#preview-heading")).toHaveText("Reading controls · 480 × 800");
  await capture("dither-drawer");
  await page.keyboard.press("Escape");
  await page.keyboard.press("Escape");
  await page.keyboard.press("Escape");
  await page.keyboard.press("ArrowDown");
  await page.keyboard.press("Enter");
  await expect(page.locator("#preview-heading")).toHaveText("File browser · 480 × 800");
  await capture("dither-files");
  await page.keyboard.press("Escape");
  await page.keyboard.press("ArrowDown");
  await page.keyboard.press("Enter");
  await expect(page.locator("#preview-heading")).toHaveText("Reader settings · 480 × 800");
  for (let row = 0; row < 3; row += 1) await page.keyboard.press("ArrowDown");
  await capture("dither-settings-preview");
});
