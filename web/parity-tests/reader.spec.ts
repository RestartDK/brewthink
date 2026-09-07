import { expect, test, type Page } from "@playwright/test";
import { readFile } from "node:fs/promises";
import path from "node:path";
import { captureFrame } from "../tests/capture-frame";

const artifacts = path.resolve("../artifacts/simulator-parity");
const defaultPreferences = 65_792;
const headings = {
  reader: "EPUB reader",
  "reader-drawer": "Reading controls",
  sleep: "Retained sleep",
};

async function packedFrame(page: Page): Promise<Buffer> {
  const bytes = await page.locator("#display").evaluate((element) => {
    if (!(element instanceof HTMLCanvasElement)) throw new Error("display must be a canvas");
    if (element.width !== 480 || element.height !== 800) throw new Error("display must be native size");
    const context = element.getContext("2d");
    if (context === null) throw new Error("display needs a 2D context");
    const rgba = context.getImageData(0, 0, 480, 800).data;
    const planeBytes = 48_000;
    const packed = new Uint8Array(planeBytes * 2);
    for (let byte = 0; byte < planeBytes; byte += 1) {
      let low = 0;
      let high = 0;
      for (let bit = 0; bit < 8; bit += 1) {
        const pixel = byte * 8 + bit;
        const offset = pixel * 4;
        const tone = rgba[offset];
        if ((tone !== 0 && tone !== 85 && tone !== 170 && tone !== 255)
          || rgba[offset + 1] !== tone || rgba[offset + 2] !== tone || rgba[offset + 3] !== 255) {
          throw new Error(`unexpected neutral display palette at pixel ${pixel}`);
        }
        const level = tone / 85;
        if ((level & 1) !== 0) low |= 0x80 >> bit;
        if ((level & 2) !== 0) high |= 0x80 >> bit;
      }
      packed[byte] = low;
      packed[byte + planeBytes] = high;
    }
    return Array.from(packed);
  });
  return Buffer.from(bytes);
}

async function expectFrame(page: Page, fixture: string, reference: string): Promise<void> {
  const expected = await readFile(path.join(artifacts, fixture, `${reference}.bin`));
  expect(expected.length).toBe(96_000);
  const actual = await packedFrame(page);
  const mismatch = actual.findIndex((byte, index) => byte !== expected[index]);
  expect(mismatch, `${fixture}/${reference} differs at packed byte ${mismatch}`).toBe(-1);
}

function unsigned(value: string | undefined): number {
  const parsed = Number(value);
  if (value === undefined || !/^\d+$/.test(value) || !Number.isSafeInteger(parsed) || parsed > 0xffff_ffff) {
    throw new Error(`invalid oracle integer: ${value}`);
  }
  return parsed;
}

async function ready(page: Page): Promise<void> {
  await page.goto("/");
  await expect(page.locator("#frame-payload")).toHaveText("96,000 bytes · 4 tones");
  const module = page.locator('script[type="module"]');
  await expect(module).toHaveCount(1);
  await expect(module).toHaveAttribute("src", /^\/assets\/.+\.js$/);
}

async function loadShelf(page: Page, fixture: string): Promise<void> {
  await ready(page);
  await page.locator("#epub-file").setInputFiles(path.join(artifacts, "fixtures", `${fixture}.epub`));
  await expect(page.locator("#file-summary")).toHaveText(`${fixture}.epub`);
  await page.keyboard.press("Enter");
  await expect(page.locator("#selected-title")).toHaveText("Parity &amp; literal");
}

async function openReader(page: Page, fixture: string, hasCover: boolean): Promise<void> {
  await loadShelf(page, fixture);
  await page.keyboard.press("Enter");
  if (hasCover) {
    await expect(page.locator("#preview-heading")).toContainText("Book cover");
    await page.keyboard.press("Enter");
  }
  await expect(page.locator("#preview-heading")).toContainText(headings.reader);
}

for (const variant of [0, 1]) {
  test(`matches every native page and chapter transition for typography ${variant}`, async ({ page }) => {
    const rows = (await readFile(path.join(artifacts, "text/pages.txt"), "utf8")).trim().split("\n").map((row) => {
      const values = row.split(" ");
      if (values.length !== 3) throw new Error(`invalid oracle row: ${row}`);
      const packed = unsigned(values[0]);
      const spine = unsigned(values[1]);
      const count = unsigned(values[2]);
      if (count === 0 || spine >= 64) throw new Error(`invalid bounded page case: ${row}`);
      return { packed, spine, count };
    });
    const preferences = [...new Set(rows.map((row) => row.packed))][variant];
    if (preferences === undefined) throw new Error("oracle preference case is missing");
    const cases = rows.filter((row) => row.packed === preferences);
    expect(cases.map((entry) => entry.spine)).toEqual(Array.from({ length: cases.length }, (_, index) => index));
    await page.addInitScript((packed) => localStorage.setItem("brewthink.reader-preferences.v1", String(packed)), preferences);
    await openReader(page, "text", true);
    for (const entry of cases) {
      for (let index = 0; index < entry.count; index += 1) {
        await expect(page.locator("#selection-position")).toHaveText(`Chapter ${entry.spine + 1} / ${cases.length}`);
        await expect(page.locator("#view-position")).toHaveText(`${index + 1} / ${entry.count}`);
        await expectFrame(page, "text", `${preferences}-${entry.spine}-${index}`);
        if (entry.spine === 0 && index === 0) {
          await captureFrame(page, path.join(artifacts, `reader-${variant}.png`));
          await page.keyboard.press("p");
          await expect(page.locator("#preview-heading")).toContainText(headings.sleep);
          await expectFrame(page, "text", "sleep");
          await page.getByRole("button", { name: "Wake", exact: true }).click();
          await expectFrame(page, "text", `${preferences}-0-0`);
        }
        await page.keyboard.press("ArrowRight");
      }
    }
  });
}

for (const fixture of ["text", "jpeg", "frame-limit"]) {
  test(`${fixture} keeps native original-resolution opening and sleep pixels`, async ({ page }) => {
    await loadShelf(page, fixture);
    await page.keyboard.press("Enter");
    await expect(page.locator("#preview-heading")).toContainText("Book cover");
    await expectFrame(page, fixture, `opening-${defaultPreferences}`);
    const opening = await packedFrame(page);
    await captureFrame(page, path.join(artifacts, `${fixture}-opening.png`));
    await page.keyboard.press("Escape");
    await expect(page.locator("#selected-title")).toHaveText("Parity &amp; literal");
    await page.keyboard.press("Enter");
    await expectFrame(page, fixture, `opening-${defaultPreferences}`);
    await page.keyboard.press("Enter");
    await expectFrame(page, fixture, `${defaultPreferences}-0-0`);
    await page.keyboard.press("p");
    await expect(page.locator("#preview-heading")).toContainText(headings.sleep);
    await expectFrame(page, fixture, "sleep");
    expect(await packedFrame(page)).toEqual(opening);
    await captureFrame(page, path.join(artifacts, `${fixture}-sleep.png`));
    await page.getByRole("button", { name: "Wake", exact: true }).click();
    await expectFrame(page, fixture, `${defaultPreferences}-0-0`);
  });
}

for (const fixture of ["text", "ncx", "malformed-nav", "no-nav"]) {
  test(`${fixture} matches native drawer drafts, cancel, jumps, endpoints, reflow and wake`, async ({ page }) => {
    const warnings: string[] = [];
    page.on("console", (entry) => { if (entry.type() === "warning") warnings.push(entry.text()); });
    await openReader(page, fixture, fixture === "text");
    if (fixture === "malformed-nav") expect(warnings.join("\n")).toContain("Malformed");
    const trace = (await readFile(path.join(artifacts, fixture, "drawer.txt"), "utf8")).trim().split("\n");
    expect(trace.length).toBe(19);
    for (const row of trace) {
      const fields = row.split("\t");
      if (fields.length !== 7) throw new Error(`invalid oracle trace: ${row}`);
      const [name, keys, screen, chapterText, pageText, countText, packedText] = fields;
      if (name === undefined || keys === undefined || !/^[a-z-]+$/.test(name)
        || (screen !== "reader" && screen !== "reader-drawer" && screen !== "sleep")) {
        throw new Error(`invalid oracle trace state: ${row}`);
      }
      for (const key of keys.split(",")) {
        if (!["Enter", "Escape", "p", "ArrowDown", "ArrowLeft", "ArrowRight", "ArrowUp"].includes(key)) {
          throw new Error(`invalid oracle input: ${key}`);
        }
        await page.keyboard.press(key);
      }
      await expect(page.locator("#preview-heading")).toContainText(headings[screen]);
      if (screen !== "sleep") {
        await expect(page.locator("#selection-position")).toHaveText(`Chapter ${unsigned(chapterText) + 1} / 2`);
        await expect(page.locator("#view-position")).toHaveText(`${unsigned(pageText) + 1} / ${unsigned(countText)}`);
      }
      expect(await page.evaluate(() => localStorage.getItem("brewthink.reader-preferences.v1"))).toBe(String(unsigned(packedText)));
      await expectFrame(page, fixture, name);
      if (name === "chapter-row" || name === "chapter-next") {
        const index = name === "chapter-row" ? 0 : 1;
        const label = fixture === "text" || fixture === "ncx"
          ? (index === 0 ? "Opening" : "Closing") : `Chapter ${index + 1}`;
        await expect(page.locator("#selected-creator")).toHaveText(`Chapter: ${label}`);
      }
      if (name === "end-position") await expect(page.locator("#selected-creator")).toHaveText("Book position: 100%");
      if (name === "start-position") await expect(page.locator("#selected-creator")).toHaveText("Book position: 0%");
      if (name === "chapter-next" || name === "type-staged") await captureFrame(page, path.join(artifacts, `${fixture}-${name}.png`));
    }
    await page.getByRole("button", { name: "Wake", exact: true }).click();
    await expectFrame(page, fixture, "awake");
  });
}

for (const fixture of ["shelf-only-cover", "shelf-limit"]) {
  test(`${fixture} preserves the shelf image but skips opening and uses native sleep fallback`, async ({ page }) => {
    await loadShelf(page, "text");
    const shelf = await packedFrame(page);
    await loadShelf(page, fixture);
    expect(await packedFrame(page)).toEqual(shelf);
    await captureFrame(page, path.join(artifacts, `${fixture}-shelf.png`));
    await page.keyboard.press("Enter");
    await expect(page.locator("#preview-heading")).toContainText(headings.reader);
    await expectFrame(page, fixture, `opening-${defaultPreferences}`);
    await page.keyboard.press("p");
    await expect(page.locator("#selected-title")).toHaveText("Brewthink");
    await expectFrame(page, fixture, "sleep");
  });
}

for (const { fixture, reason } of [
  { fixture: "malformed", reason: "Malformed" },
  { fixture: "oversized-chapter", reason: "ResourceTooLarge" },
  { fixture: "too-many-chapters", reason: "TooManySpineItems" },
]) {
  test(`rejects ${fixture} at the device boundary`, async ({ page }) => {
    await ready(page);
    await page.locator("#epub-file").setInputFiles(path.join(artifacts, "fixtures", `${fixture}.epub`));
    await expect(page.locator("#selected-title")).toHaveText("EPUB rejected");
    await expect(page.locator("#message")).toContainText(reason);
    await page.getByRole("button", { name: "Reset sample" }).click();
    await expect(page.locator("#selected-title")).toHaveText("Books");
  });
}

for (const fixture of ["no-cover", "oversized-cover", "compressed-oversized-cover", "unsupported-cover", "broken-cover", "broken-jpeg"]) {
  test(`${fixture} leaves the book readable with native fallback pixels`, async ({ page }) => {
    await openReader(page, fixture, false);
    await expectFrame(page, fixture, `opening-${defaultPreferences}`);
    await page.keyboard.press("p");
    await expect(page.locator("#selected-title")).toHaveText("Brewthink");
    await expectFrame(page, fixture, "sleep");
  });
}
