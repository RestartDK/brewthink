import init, {
  DitherMode,
  FitMode,
  render_image,
  renderer_version,
} from "./generated/brewthink_web.js";
import "./style.css";

const WIDTH = 480;
const HEIGHT = 800;
const FRAME_BYTES = 48_000;
const INK_SHADE = { red: 27, green: 27, blue: 24 };
const PAPER_SHADE = { red: 230, green: 227, blue: 211 };
const integerFormat = new Intl.NumberFormat("en-US");
const byteFormat = new Intl.NumberFormat("en-US", {
  minimumFractionDigits: 1,
  maximumFractionDigits: 1,
});

type SourceImage = Readonly<{
  name: string;
  bytes: Uint8Array;
}>;

type FrameDetails = Readonly<{
  sourceWidth: number;
  sourceHeight: number;
  contentWidth: number;
  contentHeight: number;
  blackPixels: number;
  payloadBytes: number;
}>;

type ViewState =
  | Readonly<{ kind: "empty" }>
  | Readonly<{ kind: "loading"; fileName: string }>
  | Readonly<{ kind: "error"; message: string }>
  | Readonly<{ kind: "ready"; fileName: string; details: FrameDetails }>;

const app = document.querySelector("#app");
if (!(app instanceof HTMLDivElement)) {
  throw new Error("Missing application root");
}

app.innerHTML = `
  <header class="topbar">
    <div class="product-mark" aria-label="Brewthink X4 Simulator">
      <span class="wordmark">BREWTHINK</span>
      <span class="product-divider" aria-hidden="true">/</span>
      <span class="product-name">X4 simulator</span>
    </div>
    <div class="runtime-status" id="runtime-status" role="status" aria-live="polite">
      <span class="status-light" aria-hidden="true"></span>
      <span id="runtime-label">Loading Rust renderer</span>
    </div>
  </header>

  <main class="workspace" id="simulator">
    <section class="device-stage" aria-labelledby="preview-heading">
      <div class="stage-heading">
        <div>
          <p class="eyebrow">Logical display</p>
          <h1 id="preview-heading">480 × 800 portrait</h1>
        </div>
        <span class="rotation-label">270° panel rotation</span>
      </div>

      <div class="reader" aria-label="Xteink X4 display preview">
        <div class="reader-brand" aria-hidden="true">XTEINK</div>
        <div class="display-bezel">
          <canvas
            id="display"
            width="480"
            height="800"
            role="img"
            aria-label="Empty X4 e-paper framebuffer"
          ></canvas>
          <div class="display-placeholder" id="display-placeholder">
            <span class="placeholder-grid" aria-hidden="true"></span>
            <p id="placeholder-title">No frame loaded</p>
            <span id="placeholder-detail">Select an image to run the Rust renderer.</span>
          </div>
        </div>
        <div class="reader-footer" aria-hidden="true">
          <span>480 × 800</span>
          <span>1-BIT E-PAPER</span>
        </div>
      </div>
    </section>

    <aside class="inspector" aria-labelledby="inspector-heading">
      <div class="inspector-heading">
        <p class="eyebrow">Frame preparation</p>
        <h2 id="inspector-heading">Render source</h2>
      </div>

      <section class="control-section source-section" aria-labelledby="source-heading">
        <div class="section-heading">
          <h3 id="source-heading">Source image</h3>
          <span>JPEG · PNG · BMP · PNM</span>
        </div>
        <label class="drop-zone" id="drop-zone" for="source-file">
          <input
            class="file-input"
            id="source-file"
            type="file"
            accept="image/jpeg,image/png,image/bmp,.pnm,.ppm,.pgm,.pbm"
            aria-describedby="format-help"
            disabled
          />
          <span class="drop-action">Choose image</span>
          <span class="drop-hint" id="format-help">or drop a file here</span>
        </label>
        <p class="file-summary" id="file-summary">No source selected</p>
      </section>

      <section class="control-section" aria-labelledby="fit-heading">
        <div class="section-heading">
          <h3 id="fit-heading">Image fit</h3>
          <span>480 × 800 target</span>
        </div>
        <fieldset class="segmented-control" id="fit-control">
          <legend class="visually-hidden">Image fit</legend>
          <label>
            <input type="radio" name="fit" value="contain" checked />
            <span>Contain</span>
          </label>
          <label>
            <input type="radio" name="fit" value="cover" />
            <span>Cover</span>
          </label>
        </fieldset>
      </section>

      <section class="control-section" aria-labelledby="dither-heading">
        <div class="section-heading">
          <h3 id="dither-heading">Dither</h3>
          <span>1-bit output</span>
        </div>
        <fieldset class="segmented-control" id="dither-control">
          <legend class="visually-hidden">Dither method</legend>
          <label>
            <input type="radio" name="dither" value="ordered" checked />
            <span>Ordered 4 × 4</span>
          </label>
          <label>
            <input type="radio" name="dither" value="threshold" />
            <span>Threshold</span>
          </label>
        </fieldset>
      </section>

      <section class="control-section output-section" aria-labelledby="output-heading">
        <div class="section-heading">
          <h3 id="output-heading">Packed frame</h3>
          <span id="frame-state">Waiting</span>
        </div>
        <dl class="frame-facts">
          <div>
            <dt>Source</dt>
            <dd id="source-size">—</dd>
          </div>
          <div>
            <dt>Rendered content</dt>
            <dd id="content-size">—</dd>
          </div>
          <div>
            <dt>Black pixels</dt>
            <dd id="black-pixels">—</dd>
          </div>
          <div>
            <dt>Payload</dt>
            <dd id="payload-size">—</dd>
          </div>
        </dl>
        <button class="download-button" id="download-frame" type="button" disabled>
          Download .frame.bin
        </button>
      </section>

      <div class="message" id="message" role="status" aria-live="polite">
        The browser uses the same Rust scaler and ditherer as firmware preparation.
      </div>
    </aside>
  </main>
`;

const canvas = requireCanvas("#display");
const context = requireCanvasContext(canvas);

const runtimeStatus = requireElement("#runtime-status");
const runtimeLabel = requireElement("#runtime-label");
const fileInput = requireInput("#source-file");
const dropZone = requireElement("#drop-zone");
const fileSummary = requireElement("#file-summary");
const placeholder = requireElement("#display-placeholder");
const placeholderTitle = requireElement("#placeholder-title");
const placeholderDetail = requireElement("#placeholder-detail");
const frameState = requireElement("#frame-state");
const sourceSize = requireElement("#source-size");
const contentSize = requireElement("#content-size");
const blackPixels = requireElement("#black-pixels");
const payloadSize = requireElement("#payload-size");
const downloadButton = requireButton("#download-frame");
const message = requireElement("#message");
const fitInputs = requireRadioInputs('input[name="fit"]');
const ditherInputs = requireRadioInputs('input[name="dither"]');

let source: SourceImage | null = null;
let packedFrame: Uint8Array | null = null;
let viewState: ViewState = { kind: "empty" };
let loadGeneration = 0;

drawPaper(context);
renderState();

fileInput.addEventListener("change", () => {
  const file = fileInput.files?.item(0);
  if (file !== null && file !== undefined) {
    void loadFile(file);
  }
});

dropZone.addEventListener("dragenter", handleDragEnter);
dropZone.addEventListener("dragover", handleDragEnter);
dropZone.addEventListener("dragleave", handleDragLeave);
dropZone.addEventListener("drop", (event) => {
  event.preventDefault();
  dropZone.classList.remove("is-dragging");
  const file = event.dataTransfer?.files.item(0);
  if (file !== null && file !== undefined && !fileInput.disabled) {
    void loadFile(file);
  }
});

for (const input of [...fitInputs, ...ditherInputs]) {
  input.addEventListener("change", () => {
    if (source !== null) {
      renderSource(source);
    }
  });
}

downloadButton.addEventListener("click", () => {
  if (packedFrame === null || source === null) {
    return;
  }

  const payload = new ArrayBuffer(packedFrame.byteLength);
  new Uint8Array(payload).set(packedFrame);
  const blob = new Blob([payload], { type: "application/octet-stream" });
  const link = document.createElement("a");
  const url = URL.createObjectURL(blob);
  link.href = url;
  link.download = `${fileStem(source.name)}.frame.bin`;
  document.body.append(link);
  link.click();
  link.remove();
  window.setTimeout(() => URL.revokeObjectURL(url), 0);
});

void initializeRenderer();

async function initializeRenderer(): Promise<void> {
  try {
    await init();
    runtimeStatus.classList.add("is-ready");
    runtimeLabel.textContent = `Rust/WASM ${renderer_version()}`;
    fileInput.disabled = false;
  } catch (error: unknown) {
    runtimeStatus.classList.add("is-error");
    runtimeLabel.textContent = "Renderer unavailable";
    viewState = { kind: "error", message: errorMessage(error) };
    renderState();
  }
}

async function loadFile(file: File): Promise<void> {
  const generation = ++loadGeneration;
  viewState = { kind: "loading", fileName: file.name };
  renderState();

  try {
    const bytes = new Uint8Array(await file.arrayBuffer());
    if (generation !== loadGeneration) {
      return;
    }
    source = { name: file.name, bytes };
    renderSource(source);
  } catch (error: unknown) {
    viewState = { kind: "error", message: errorMessage(error) };
    packedFrame = null;
    renderState();
  } finally {
    fileInput.value = "";
  }
}

function renderSource(image: SourceImage): void {
  viewState = { kind: "loading", fileName: image.name };
  renderState();

  try {
    const frame = render_image(image.bytes, selectedFit(), selectedDither());
    const pixels = frame.pixels();
    const details: FrameDetails = {
      sourceWidth: frame.source_width,
      sourceHeight: frame.source_height,
      contentWidth: frame.content_width,
      contentHeight: frame.content_height,
      blackPixels: frame.black_pixels,
      payloadBytes: frame.payload_bytes,
    };
    frame.free();

    drawFrame(context, pixels);
    packedFrame = pixels;
    viewState = { kind: "ready", fileName: image.name, details };
  } catch (error: unknown) {
    packedFrame = null;
    viewState = { kind: "error", message: errorMessage(error) };
  }

  renderState();
}

function renderState(): void {
  message.classList.toggle("is-error", viewState.kind === "error");

  switch (viewState.kind) {
    case "empty":
      placeholder.hidden = false;
      placeholderTitle.textContent = "No frame loaded";
      placeholderDetail.textContent = "Select an image to run the Rust renderer.";
      fileSummary.textContent = "No source selected";
      frameState.textContent = "Waiting";
      message.textContent =
        "The browser uses the same Rust scaler and ditherer as firmware preparation.";
      clearFacts();
      break;
    case "loading":
      placeholder.hidden = false;
      placeholderTitle.textContent = "Preparing frame";
      placeholderDetail.textContent = viewState.fileName;
      fileSummary.textContent = viewState.fileName;
      frameState.textContent = "Rendering";
      message.textContent = "Decoding and rendering inside Rust/WASM…";
      break;
    case "error":
      placeholder.hidden = false;
      placeholderTitle.textContent = "Could not render image";
      placeholderDetail.textContent = "Choose a supported image and try again.";
      frameState.textContent = "Error";
      message.textContent = viewState.message;
      clearFacts();
      break;
    case "ready": {
      const { details } = viewState;
      placeholder.hidden = true;
      canvas.setAttribute(
        "aria-label",
        `${viewState.fileName}, rendered as a 480 by 800 monochrome X4 framebuffer`,
      );
      fileSummary.textContent = viewState.fileName;
      frameState.textContent = "Ready";
      sourceSize.textContent = `${details.sourceWidth} × ${details.sourceHeight}`;
      contentSize.textContent = `${details.contentWidth} × ${details.contentHeight}`;
      blackPixels.textContent = integerFormat.format(details.blackPixels);
      payloadSize.textContent = `${byteFormat.format(details.payloadBytes / 1024)} KiB`;
      message.textContent = "Frame ready. The download is the raw 48,000-byte page payload.";
      break;
    }
  }

  downloadButton.disabled = viewState.kind !== "ready";
}

function clearFacts(): void {
  sourceSize.textContent = "—";
  contentSize.textContent = "—";
  blackPixels.textContent = "—";
  payloadSize.textContent = "—";
}

function selectedFit(): FitMode {
  const selected = checkedValue(fitInputs);
  if (selected === "contain") {
    return FitMode.Contain;
  }
  if (selected === "cover") {
    return FitMode.Cover;
  }
  throw new Error(`Unknown fit mode: ${selected}`);
}

function selectedDither(): DitherMode {
  const selected = checkedValue(ditherInputs);
  if (selected === "ordered") {
    return DitherMode.Ordered;
  }
  if (selected === "threshold") {
    return DitherMode.Threshold;
  }
  throw new Error(`Unknown dither mode: ${selected}`);
}

function checkedValue(inputs: readonly HTMLInputElement[]): string {
  const selected = inputs.find((input) => input.checked);
  if (selected === undefined) {
    throw new Error("A rendering option must be selected");
  }
  return selected.value;
}

function drawPaper(target: CanvasRenderingContext2D): void {
  target.fillStyle = "rgb(230 227 211)";
  target.fillRect(0, 0, WIDTH, HEIGHT);
}

function drawFrame(target: CanvasRenderingContext2D, pixels: Uint8Array): void {
  if (pixels.length !== FRAME_BYTES) {
    throw new Error(`Expected ${FRAME_BYTES} frame bytes, received ${pixels.length}`);
  }

  const imageData = target.createImageData(WIDTH, HEIGHT);
  let destination = 0;

  for (const byte of pixels) {
    for (let bit = 7; bit >= 0; bit -= 1) {
      const isWhite = (byte & (1 << bit)) !== 0;
      const shade = isWhite ? PAPER_SHADE : INK_SHADE;
      imageData.data[destination] = shade.red;
      imageData.data[destination + 1] = shade.green;
      imageData.data[destination + 2] = shade.blue;
      imageData.data[destination + 3] = 255;
      destination += 4;
    }
  }

  target.putImageData(imageData, 0, 0);
}

function handleDragEnter(event: DragEvent): void {
  event.preventDefault();
  if (!fileInput.disabled) {
    dropZone.classList.add("is-dragging");
  }
}

function handleDragLeave(event: DragEvent): void {
  if (event.relatedTarget instanceof Node && dropZone.contains(event.relatedTarget)) {
    return;
  }
  dropZone.classList.remove("is-dragging");
}

function fileStem(fileName: string): string {
  const withoutExtension = fileName.replace(/\.[^.]+$/, "");
  const safeName = withoutExtension.replace(/[^a-zA-Z0-9_-]+/g, "-");
  return safeName.length === 0 ? "brewthink-page" : safeName;
}

function errorMessage(error: unknown): string {
  if (error instanceof Error) {
    return error.message;
  }
  return String(error);
}

function requireElement(selector: string): HTMLElement {
  const element = document.querySelector(selector);
  if (!(element instanceof HTMLElement)) {
    throw new Error(`Missing element: ${selector}`);
  }
  return element;
}

function requireInput(selector: string): HTMLInputElement {
  const element = document.querySelector(selector);
  if (!(element instanceof HTMLInputElement)) {
    throw new Error(`Missing input: ${selector}`);
  }
  return element;
}

function requireButton(selector: string): HTMLButtonElement {
  const element = document.querySelector(selector);
  if (!(element instanceof HTMLButtonElement)) {
    throw new Error(`Missing button: ${selector}`);
  }
  return element;
}

function requireCanvas(selector: string): HTMLCanvasElement {
  const element = document.querySelector(selector);
  if (!(element instanceof HTMLCanvasElement)) {
    throw new Error(`Missing canvas: ${selector}`);
  }
  return element;
}

function requireCanvasContext(canvasElement: HTMLCanvasElement): CanvasRenderingContext2D {
  const canvasContext = canvasElement.getContext("2d");
  if (canvasContext === null) {
    throw new Error("Canvas 2D rendering is unavailable");
  }
  return canvasContext;
}

function requireRadioInputs(selector: string): readonly HTMLInputElement[] {
  const elements = document.querySelectorAll(selector);
  const inputs = Array.from(elements).filter(
    (element): element is HTMLInputElement => element instanceof HTMLInputElement,
  );
  if (inputs.length === 0 || inputs.length !== elements.length) {
    throw new Error(`Missing radio inputs: ${selector}`);
  }
  return inputs;
}
