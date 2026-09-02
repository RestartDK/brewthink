import init, {
  RenderedFrame,
  WebInput,
  WebLibrary,
  renderer_version,
} from "./generated/brewthink_web.js";
import "./style.css";

const WIDTH = 480;
const HEIGHT = 800;
const FRAME_BYTES = 48_000;
const MAX_EPUB_BYTES = 32 * 1024 * 1024;
const READER_PREFERENCES_KEY = "brewthink.reader-preferences.v1";
const INVALID_READER_PREFERENCES = 0xffff_ffff;
const INK_SHADE = { red: 27, green: 27, blue: 24 };
const PAPER_SHADE = { red: 230, green: 227, blue: 211 };
const integerFormat = new Intl.NumberFormat("en-US");

type Screen = "home" | "library" | "files" | "settings" | "reader" | "sleep" | "error";

type ViewState =
  | Readonly<{ kind: "booting" }>
  | Readonly<{ kind: "rendering"; sourceName: string }>
  | Readonly<{
      kind: "ready";
      sourceName: string;
      screen: Screen;
      title: string;
      creator: string;
      selected: number;
      itemCount: number;
      page: number;
      pageCount: number;
      chapter: number;
      chapterCount: number;
    }>
  | Readonly<{ kind: "error"; sourceName: string; message: string }>;

const app = document.querySelector("#app");
if (!(app instanceof HTMLDivElement)) {
  throw new Error("Missing application root");
}

app.innerHTML = `
  <header class="topbar">
    <div class="product-mark" aria-label="Brewthink X4 Simulator">
      <span class="wordmark">BREWTHINK</span>
      <span class="product-divider" aria-hidden="true">/</span>
      <span class="product-name">Reader simulator</span>
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
          <p class="eyebrow">Shared application frame</p>
          <h1 id="preview-heading">Starting · 480 × 800</h1>
        </div>
        <span class="rotation-label">X4 portrait view</span>
      </div>

      <div class="reader" aria-label="Xteink X4 display preview">
        <div class="reader-brand" aria-hidden="true">XTEINK</div>
        <div class="display-bezel">
          <canvas
            id="display"
            width="480"
            height="800"
            role="img"
            aria-label="Brewthink application loading"
          ></canvas>
          <div class="display-placeholder" id="display-placeholder">
            <span class="placeholder-grid" aria-hidden="true"></span>
            <p id="placeholder-title">Starting renderer</p>
            <span id="placeholder-detail">Preparing the shared Rust application frame.</span>
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
        <div>
          <p class="eyebrow">Complete product loop</p>
          <h2 id="inspector-heading">Home · books · files · settings · read</h2>
        </div>
        <span class="build-label">WASM</span>
      </div>

      <section class="control-section source-section" aria-labelledby="source-heading">
        <div class="section-heading">
          <h3 id="source-heading">Library source</h3>
          <span>DRM-free EPUB</span>
        </div>
        <label class="drop-zone" id="drop-zone" for="epub-file">
          <input
            class="file-input"
            id="epub-file"
            type="file"
            accept="application/epub+zip,.epub"
            aria-describedby="format-help"
            disabled
          />
          <span class="drop-action">Open EPUB</span>
          <span class="drop-hint" id="format-help">or drop one here · read only</span>
        </label>
        <div class="source-summary">
          <p class="file-summary" id="file-summary">Built-in public-domain sample</p>
          <button class="text-button" id="reset-library" type="button" disabled>Reset sample</button>
        </div>
      </section>

      <section class="control-section selected-section" aria-labelledby="selected-heading">
        <div class="section-heading">
          <h3 id="selected-heading">Current selection</h3>
          <span id="selection-position">—</span>
        </div>
        <div>
          <p class="selected-title" id="selected-title">Waiting for renderer</p>
          <p class="selected-creator" id="selected-creator">—</p>
        </div>
        <dl class="frame-facts">
          <div>
            <dt id="page-label">View</dt>
            <dd id="view-position">—</dd>
          </div>
          <div>
            <dt>Frame payload</dt>
            <dd>${integerFormat.format(FRAME_BYTES)} bytes</dd>
          </div>
        </dl>
      </section>

      <section class="control-section input-section" aria-labelledby="input-heading">
        <div class="section-heading">
          <h3 id="input-heading">Device input</h3>
          <span>Arrows · Enter · Esc · P</span>
        </div>
        <div class="device-controls">
          <div class="direction-pad" aria-label="Application navigation">
            <button class="key key-up" type="button" data-input="up" aria-label="Move up">↑</button>
            <button class="key key-left" type="button" data-input="left" aria-label="Move left">←</button>
            <span class="key-center" aria-hidden="true"></span>
            <button class="key key-right" type="button" data-input="right" aria-label="Move right">→</button>
            <button class="key key-down" type="button" data-input="down" aria-label="Move down">↓</button>
          </div>
          <button class="confirm-button" id="confirm-selection" type="button" disabled>
            <span id="confirm-label">Confirm</span>
            <small id="confirm-hint">Open selected book</small>
          </button>
        </div>
        <div class="system-controls">
          <button class="secondary-button" id="back-button" type="button" disabled>Back</button>
          <button class="secondary-button power-button" id="power-button" type="button" disabled>
            Sleep
          </button>
        </div>
      </section>

      <div class="message" id="message" role="status" aria-live="polite">
        Shared Rust owns the home menu, books, files, settings, reading, and sleep.
      </div>
    </aside>
  </main>
`;

const canvas = requireCanvas("#display");
const context = requireCanvasContext(canvas);
const runtimeStatus = requireElement("#runtime-status");
const runtimeLabel = requireElement("#runtime-label");
const previewHeading = requireElement("#preview-heading");
const fileInput = requireInput("#epub-file");
const dropZone = requireElement("#drop-zone");
const fileSummary = requireElement("#file-summary");
const placeholder = requireElement("#display-placeholder");
const placeholderTitle = requireElement("#placeholder-title");
const placeholderDetail = requireElement("#placeholder-detail");
const selectedTitle = requireElement("#selected-title");
const selectedCreator = requireElement("#selected-creator");
const selectionPosition = requireElement("#selection-position");
const pageLabel = requireElement("#page-label");
const viewPosition = requireElement("#view-position");
const resetButton = requireButton("#reset-library");
const confirmButton = requireButton("#confirm-selection");
const confirmLabel = requireElement("#confirm-label");
const confirmHint = requireElement("#confirm-hint");
const backButton = requireButton("#back-button");
const powerButton = requireButton("#power-button");
const message = requireElement("#message");
const directionButtons = requireButtons("[data-input]");

let library: WebLibrary | null = null;
let sourceName = "Built-in public-domain sample";
let viewState: ViewState = { kind: "booting" };
let loadGeneration = 0;

drawPaper(context);
renderState();

fileInput.addEventListener("change", () => {
  const file = fileInput.files?.item(0);
  if (file !== null && file !== undefined) {
    void loadEpub(file);
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
    void loadEpub(file);
  }
});

for (const button of directionButtons) {
  button.addEventListener("click", () => {
    const input = inputFromValue(button.dataset.input);
    if (input !== null) {
      sendInput(input);
    }
  });
}

document.addEventListener("keydown", (event) => {
  if (event.target instanceof HTMLInputElement || event.target instanceof HTMLButtonElement) {
    return;
  }
  const input = inputFromKey(event.key);
  if (input !== null) {
    event.preventDefault();
    sendInput(input);
  }
});

resetButton.addEventListener("click", () => {
  replaceLibrary(
    WebLibrary.withPreferences(readStoredPreferences()),
    "Built-in public-domain sample",
  );
});

confirmButton.addEventListener("click", () => sendInput(WebInput.Confirm));
backButton.addEventListener("click", () => sendInput(WebInput.Back));
powerButton.addEventListener("click", () => sendInput(WebInput.Power));

void initializeRenderer();

async function initializeRenderer(): Promise<void> {
  try {
    await init();
    runtimeStatus.classList.add("is-ready");
    runtimeLabel.textContent = `Rust/WASM ${renderer_version()}`;
    fileInput.disabled = false;
    replaceLibrary(
      WebLibrary.withPreferences(readStoredPreferences()),
      "Built-in public-domain sample",
    );
  } catch (error: unknown) {
    runtimeStatus.classList.add("is-error");
    runtimeLabel.textContent = "Renderer unavailable";
    viewState = {
      kind: "error",
      sourceName,
      message: errorMessage(error),
    };
    renderState();
  }
}

async function loadEpub(file: File): Promise<void> {
  const generation = ++loadGeneration;
  viewState = { kind: "rendering", sourceName: file.name };
  renderState();

  try {
    if (file.size > MAX_EPUB_BYTES) {
      throw new Error("EPUB exceeds the 32 MiB simulator limit");
    }
    const bytes = new Uint8Array(await file.arrayBuffer());
    if (generation !== loadGeneration) {
      return;
    }
    replaceLibrary(
      WebLibrary.fromEpub(bytes, file.name, readStoredPreferences()),
      file.name,
    );
  } catch (error: unknown) {
    viewState = {
      kind: "error",
      sourceName: file.name,
      message: errorMessage(error),
    };
    renderState();
  } finally {
    fileInput.value = "";
  }
}

function replaceLibrary(nextLibrary: WebLibrary, nextSourceName: string): void {
  library?.free();
  library = nextLibrary;
  sourceName = nextSourceName;
  renderApplication();
}

function sendInput(input: WebInput): void {
  if (library === null || viewState.kind !== "ready") {
    return;
  }
  if (viewState.screen === "sleep" && input === WebInput.Power) {
    library.wake();
  } else {
    library.input(input);
  }
  localStorage.setItem(READER_PREFERENCES_KEY, String(library.preferences));
  renderApplication();
}

function renderApplication(): void {
  if (library === null) {
    return;
  }
  viewState = { kind: "rendering", sourceName };
  renderState();

  let frame: RenderedFrame | null = null;
  try {
    frame = library.render();
    drawFrame(context, frame.pixels());
    viewState = {
      kind: "ready",
      sourceName,
      screen: parseScreen(frame.screen),
      title: frame.title,
      creator: frame.creator,
      selected: frame.selected,
      itemCount: frame.item_count,
      page: frame.page,
      pageCount: frame.page_count,
      chapter: frame.chapter,
      chapterCount: frame.chapter_count,
    };
  } catch (error: unknown) {
    viewState = { kind: "error", sourceName, message: errorMessage(error) };
  } finally {
    frame?.free();
  }
  renderState();
}

function renderState(): void {
  const isReady = viewState.kind === "ready";
  const isError = viewState.kind === "error";
  const screen = viewState.kind === "ready" ? viewState.screen : null;
  message.classList.toggle("is-error", isError || screen === "error");
  resetButton.disabled =
    viewState.kind === "booting" ||
    viewState.kind === "rendering" ||
    (isReady && sourceName === "Built-in public-domain sample");
  confirmButton.disabled = !isReady || screen === "sleep" || screen === "error";
  backButton.disabled = !isReady || screen === "home" || screen === "sleep";
  powerButton.disabled = !isReady || screen === "error";
  powerButton.textContent = screen === "sleep" ? "Wake" : "Sleep";
  for (const button of directionButtons) {
    button.disabled = !isReady || screen === "sleep" || screen === "error";
  }

  switch (viewState.kind) {
    case "booting":
      placeholder.hidden = false;
      placeholderTitle.textContent = "Starting renderer";
      placeholderDetail.textContent = "Preparing the shared Rust application frame.";
      selectedTitle.textContent = "Waiting for renderer";
      selectedCreator.textContent = "—";
      selectionPosition.textContent = "—";
      viewPosition.textContent = "—";
      break;
    case "rendering":
      placeholder.hidden = false;
      placeholderTitle.textContent = "Building frame";
      placeholderDetail.textContent = viewState.sourceName;
      fileSummary.textContent = viewState.sourceName;
      message.textContent = "Parsing bounded content and packing a 1-bit frame…";
      break;
    case "error":
      placeholder.hidden = false;
      placeholderTitle.textContent = "Could not open EPUB";
      placeholderDetail.textContent = "Choose a valid, DRM-free EPUB and try again.";
      fileSummary.textContent = viewState.sourceName;
      selectedTitle.textContent = "EPUB rejected";
      selectedCreator.textContent = "—";
      selectionPosition.textContent = "—";
      viewPosition.textContent = "—";
      message.textContent = viewState.message;
      break;
    case "ready":
      placeholder.hidden = true;
      fileSummary.textContent = viewState.sourceName;
      selectedTitle.textContent = viewState.title;
      selectedCreator.textContent = viewState.creator;
      renderReadyState(viewState);
      break;
  }
}

function renderReadyState(state: Extract<ViewState, { kind: "ready" }>): void {
  switch (state.screen) {
    case "home":
      previewHeading.textContent = "Home menu · 480 × 800";
      selectionPosition.textContent = `${state.selected + 1} / ${state.itemCount}`;
      pageLabel.textContent = "Menu";
      viewPosition.textContent = state.title;
      confirmLabel.textContent = "Open";
      confirmHint.textContent = `Go to ${state.title.toLowerCase()}`;
      message.textContent = "Choose Books, Files, or Settings with the same controls as the X4.";
      canvas.setAttribute(
        "aria-label",
        `Brewthink home menu. Selected: ${state.title}. Battery 82 percent.`,
      );
      break;
    case "library":
      previewHeading.textContent = "Library shelf · 480 × 800";
      selectionPosition.textContent = `${state.selected + 1} / ${state.itemCount}`;
      pageLabel.textContent = "Shelf page";
      viewPosition.textContent = `${state.page + 1} / ${state.pageCount}`;
      confirmLabel.textContent = "Confirm";
      confirmHint.textContent = "Open selected book";
      message.textContent =
        "Choose a cover, press Confirm, then turn pages with Left and Right.";
      canvas.setAttribute(
        "aria-label",
        `Brewthink two by two library shelf. Selected: ${state.title} by ${state.creator}.`,
      );
      break;
    case "files":
      previewHeading.textContent = "File browser · 480 × 800";
      selectionPosition.textContent = `${state.selected + 1} / ${state.itemCount}`;
      pageLabel.textContent = "File page";
      viewPosition.textContent = `${state.page + 1} / ${state.pageCount}`;
      confirmLabel.textContent = "Open EPUB";
      confirmHint.textContent = state.title;
      message.textContent = "Files shows the EPUB source names and sizes from the read-only catalog.";
      canvas.setAttribute(
        "aria-label",
        `Brewthink file browser. Selected: ${state.title}.`,
      );
      break;
    case "settings":
      previewHeading.textContent = "Reader settings · 480 × 800";
      selectionPosition.textContent = `${state.selected + 1} / ${state.itemCount}`;
      pageLabel.textContent = "Current value";
      viewPosition.textContent = state.creator;
      confirmLabel.textContent = state.title === "APPLY SETTINGS" ? "Apply" : "Next value";
      confirmHint.textContent = "Left and Right also change";
      message.textContent =
        "Reader typography changes pagination and rendering. The Brewthink wordmark stays fixed.";
      canvas.setAttribute(
        "aria-label",
        `Brewthink reader settings. Selected: ${state.title}. Value: ${state.creator}.`,
      );
      break;
    case "reader":
      previewHeading.textContent = "EPUB reader · 480 × 800";
      selectionPosition.textContent = `Chapter ${state.chapter + 1} / ${state.chapterCount}`;
      pageLabel.textContent = "Chapter page";
      viewPosition.textContent = `${state.page + 1} / ${state.pageCount}`;
      confirmLabel.textContent = "Next page";
      confirmHint.textContent = "Right and Down also turn";
      message.textContent =
        "Sleep restores the page. Typography changes reflow the chapter around its saved progress.";
      canvas.setAttribute(
        "aria-label",
        `${state.title} reader. Chapter ${state.chapter + 1} of ${state.chapterCount}, page ${state.page + 1} of ${state.pageCount}.`,
      );
      break;
    case "sleep":
      previewHeading.textContent = "Retained sleep screen · 480 × 800";
      selectionPosition.textContent = "Position retained";
      pageLabel.textContent = "Power state";
      viewPosition.textContent = "Deep sleep";
      confirmLabel.textContent = "Sleeping";
      confirmHint.textContent = "Press Wake to resume";
      message.textContent =
        "The e-paper frame remains visible without power. Wake restores the prior view and page.";
      canvas.setAttribute(
        "aria-label",
        `Brewthink sleep screen for ${state.title}. Reading position retained.`,
      );
      break;
    case "error":
      previewHeading.textContent = "Book error · 480 × 800";
      selectionPosition.textContent = "Book unavailable";
      pageLabel.textContent = "Recovery";
      viewPosition.textContent = "Back to library";
      message.textContent = "This book could not be opened. Return to the library and choose another.";
      break;
  }
}

function inputFromValue(value: string | undefined): WebInput | null {
  switch (value) {
    case "left":
      return WebInput.Left;
    case "right":
      return WebInput.Right;
    case "up":
      return WebInput.Up;
    case "down":
      return WebInput.Down;
    default:
      return null;
  }
}

function inputFromKey(key: string): WebInput | null {
  switch (key) {
    case "ArrowLeft":
      return WebInput.Left;
    case "ArrowRight":
      return WebInput.Right;
    case "ArrowUp":
      return WebInput.Up;
    case "ArrowDown":
      return WebInput.Down;
    case "Enter":
      return WebInput.Confirm;
    case "Escape":
      return WebInput.Back;
    case "p":
    case "P":
    case " ":
      return WebInput.Power;
    default:
      return null;
  }
}

function parseScreen(value: string): Screen {
  if (
    value === "home" ||
    value === "library" ||
    value === "files" ||
    value === "settings" ||
    value === "reader" ||
    value === "sleep" ||
    value === "error"
  ) {
    return value;
  }
  throw new Error(`Unknown application screen: ${value}`);
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

function readStoredPreferences(): number {
  const stored = localStorage.getItem(READER_PREFERENCES_KEY);
  if (stored === null) {
    return INVALID_READER_PREFERENCES;
  }
  const value = Number(stored);
  if (!Number.isSafeInteger(value) || value < 0 || value > 0xffff_ffff) {
    return INVALID_READER_PREFERENCES;
  }
  return value;
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

function requireButtons(selector: string): readonly HTMLButtonElement[] {
  const elements = document.querySelectorAll(selector);
  const buttons = Array.from(elements).filter(
    (element): element is HTMLButtonElement => element instanceof HTMLButtonElement,
  );
  if (buttons.length === 0 || buttons.length !== elements.length) {
    throw new Error(`Missing buttons: ${selector}`);
  }
  return buttons;
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
