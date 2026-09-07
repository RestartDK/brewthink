import { expect, test, type Page } from "@playwright/test";
import { readFile, writeFile } from "node:fs/promises";
import path from "node:path";

const artifacts = path.resolve("../artifacts/simulator-parity");

async function packedFrame(page: Page): Promise<Buffer> {
  const bytes = await page.locator("#display").evaluate((element) => {
    if (!(element instanceof HTMLCanvasElement)) throw new Error("display must be a canvas");
    const context = element.getContext("2d");
    if (context === null) throw new Error("display needs a 2D context");
    const rgba = context.getImageData(0, 0, 480, 800).data;
    const packed = new Uint8Array(48_000);
    for (let pixel = 0; pixel < 480 * 800; pixel += 1) {
      const offset = pixel * 4;
      const white = rgba[offset] === 230 && rgba[offset + 1] === 227 && rgba[offset + 2] === 211;
      const black = rgba[offset] === 27 && rgba[offset + 1] === 27 && rgba[offset + 2] === 24;
      if ((!white && !black) || rgba[offset + 3] !== 255) throw new Error("unexpected display palette");
      if (white) packed[Math.floor(pixel / 8)] |= 0x80 >> (pixel % 8);
    }
    return Array.from(packed);
  });
  return Buffer.from(bytes);
}

async function saveFrame(page: Page, fileName: string): Promise<void> {
  const data = await page.locator("#display").evaluate((element) => {
    if (!(element instanceof HTMLCanvasElement)) throw new Error("display must be a canvas");
    return element.toDataURL("image/png");
  });
  const prefix = "data:image/png;base64,";
  if (!data.startsWith(prefix)) throw new Error("display must encode a PNG");
  await writeFile(fileName, Buffer.from(data.slice(prefix.length), "base64"));
}

async function expectFrame(page: Page, reference: string): Promise<void> {
  const expected = await readFile(reference);
  const actual = await packedFrame(page);
  expect(actual.length).toBe(expected.length);
  const mismatch = actual.findIndex((byte, index) => byte !== expected[index]);
  expect(mismatch, `${path.basename(reference)} differs at packed byte ${mismatch}`).toBe(-1);
}

for (const variant of [0, 1]) {
  test(`matches every device page and chapter transition for typography ${variant}`, async ({ page }) => {
    const rows = (await readFile(path.join(artifacts, "text/pages.txt"), "utf8")).trim().split("\n").map((row) => {
      if (!/^\d+ \d+ \d+$/.test(row)) throw new Error(`invalid oracle row: ${row}`);
      const [packed, spine, count] = row.split(" ").map(Number);
      return { packed, spine, count };
    });
    const preferences = [...new Set(rows.map((row) => row.packed))][variant];
    if (preferences === undefined) throw new Error("oracle preference case is missing");
    const cases = rows.filter((row) => row.packed === preferences);
    await page.addInitScript((packed) => localStorage.setItem("brewthink.reader-preferences.v1", String(packed)), preferences);
    await page.goto("/");
    await expect(page.getByText("Rust/WASM 0.1.0")).toBeVisible();
    await page.locator("#epub-file").setInputFiles(path.join(artifacts, "text.epub"));
    await expect(page.locator("#file-summary")).toHaveText("text.epub");
    await page.keyboard.press("Enter");
    await expect(page.locator("#selected-title")).toHaveText("Parity &amp; literal");
    await page.keyboard.press("Enter");
    for (const entry of cases) {
      for (let index = 0; index < entry.count; index += 1) {
        await expect(page.locator("#selection-position")).toHaveText(`Chapter ${entry.spine + 1} / ${cases.length}`);
        await expect(page.locator("#view-position")).toHaveText(`${index + 1} / ${entry.count}`);
        await expectFrame(page, path.join(artifacts, `text/${preferences}-${entry.spine}-${index}.bin`));
        if (entry.spine === 0 && index === 0) {
          await saveFrame(page, path.join(artifacts, `reader-${variant}.png`));
          await page.keyboard.press("p");
          await expect(page.locator("#preview-heading")).toContainText("Retained sleep");
          await expectFrame(page, path.join(artifacts, "text/sleep.bin"));
          await page.getByRole("button", { name: "Wake", exact: true }).click();
          await expectFrame(page, path.join(artifacts, `text/${preferences}-0-0.bin`));
        }
        await page.keyboard.press("ArrowRight");
      }
    }
  });
}

test("JPEG cover sleep pixels match the device decoder", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByText("Rust/WASM 0.1.0")).toBeVisible();
  await page.locator("#epub-file").setInputFiles(path.join(artifacts, "jpeg.epub"));
  await expect(page.locator("#file-summary")).toHaveText("jpeg.epub");
  await page.keyboard.press("Enter");
  await page.keyboard.press("Enter");
  await expect(page.locator("#preview-heading")).toContainText("EPUB reader");
  await page.keyboard.press("p");
  await expectFrame(page, path.join(artifacts, "jpeg/sleep.bin"));
  await saveFrame(page, path.join(artifacts, "jpeg-sleep.png"));
});

for (const [fixture, reason] of [
  ["malformed", "Malformed"],
  ["oversized-chapter", "ResourceTooLarge"],
  ["too-many-chapters", "TooManySpineItems"],
]) {
  test(`rejects ${fixture} at the device boundary`, async ({ page }) => {
    await page.goto("/");
    await expect(page.getByText("Rust/WASM 0.1.0")).toBeVisible();
    await page.locator("#epub-file").setInputFiles(path.join(artifacts, `${fixture}.epub`));
    await expect(page.locator("#selected-title")).toHaveText("EPUB rejected");
    await expect(page.locator("#message")).toContainText(reason);
    await page.getByRole("button", { name: "Reset sample" }).click();
    await expect(page.locator("#selected-title")).toHaveText("BOOKS");
  });
}

for (const fixture of ["no-cover", "oversized-cover", "unsupported-cover", "broken-cover"]) {
  test(`${fixture} leaves the book readable with a sleep fallback`, async ({ page }) => {
    await page.goto("/");
    await expect(page.getByText("Rust/WASM 0.1.0")).toBeVisible();
    await page.locator("#epub-file").setInputFiles(path.join(artifacts, `${fixture}.epub`));
    await expect(page.locator("#file-summary")).toHaveText(`${fixture}.epub`);
    await page.keyboard.press("Enter");
    await page.keyboard.press("Enter");
    await expect(page.locator("#preview-heading")).toContainText("EPUB reader");
    await page.keyboard.press("p");
    await expect(page.locator("#selected-title")).toHaveText("Brewthink");
  });
}
