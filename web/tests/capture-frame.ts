import type { Page } from "@playwright/test";
import { mkdir, writeFile } from "node:fs/promises";
import { dirname } from "node:path";

const LOGICAL_SHADES = new Set([0, 85, 170, 255]);

export async function captureFrame(page: Page, filePath: string): Promise<void> {
  const encoded = await page.locator("#display").evaluate((element, shades) => {
    if (!(element instanceof HTMLCanvasElement)) throw new Error("Expected display canvas");
    if (element.width !== 480 || element.height !== 800) throw new Error("Unexpected frame dimensions");
    const context = element.getContext("2d");
    if (context === null) throw new Error("Missing display context");
    const logicalShades = new Set(shades);
    const { data } = context.getImageData(0, 0, 480, 800);
    for (let index = 0; index < data.length; index += 4) {
      const shade = data[index];
      if (
        shade === undefined ||
        !logicalShades.has(shade) ||
        data[index + 1] !== shade ||
        data[index + 2] !== shade ||
        data[index + 3] !== 255
      ) {
        throw new Error(`Invalid logical shade at ${index / 4}`);
      }
    }
    return element.toDataURL("image/png").slice("data:image/png;base64,".length);
  }, [...LOGICAL_SHADES]);
  await mkdir(dirname(filePath), { recursive: true });
  await writeFile(filePath, Buffer.from(encoded, "base64"));
}
