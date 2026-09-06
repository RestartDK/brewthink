import type { Page } from "@playwright/test";
import { writeFile } from "node:fs/promises";

export async function captureFrame(page: Page, filePath: string): Promise<void> {
  const encoded = await page.locator("#display").evaluate((element) => {
    if (!(element instanceof HTMLCanvasElement)) throw new Error("Expected display canvas");
    if (element.width !== 480 || element.height !== 800) throw new Error("Unexpected frame dimensions");
    const context = element.getContext("2d");
    if (context === null) throw new Error("Missing display context");
    const { data } = context.getImageData(0, 0, 480, 800);
    for (let index = 0; index < data.length; index += 4) {
      const shade = data[index];
      if ((shade !== 0 && shade !== 255) || data[index + 1] !== shade || data[index + 2] !== shade || data[index + 3] !== 255) {
        throw new Error(`Non-monochrome pixel at ${index / 4}`);
      }
    }
    return element.toDataURL("image/png").slice("data:image/png;base64,".length);
  });
  await writeFile(filePath, Buffer.from(encoded, "base64"));
}
