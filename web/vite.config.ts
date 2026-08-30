import { spawn, type ChildProcess } from "node:child_process";
import { fileURLToPath } from "node:url";
import { isAbsolute, join, relative, resolve, sep } from "node:path";
import { defineConfig, type Plugin, type ViteDevServer } from "vite";

const PROJECT_ROOT = fileURLToPath(new URL("..", import.meta.url));
const BUILD_SCRIPT = join(PROJECT_ROOT, "scripts/build-web-wasm.sh");
const REBUILD_EVENTS = new Set(["add", "change", "unlink"]);
const WATCHED_FILES = new Set([
  "Cargo.lock",
  "Cargo.toml",
  "build.rs",
  join(".cargo", "config.toml"),
  join("scripts", "build-web-wasm.sh"),
]);
const WATCHED_PATHS = [
  join(PROJECT_ROOT, "src"),
  ...Array.from(WATCHED_FILES, (file) => join(PROJECT_ROOT, file)),
];
const DEBOUNCE_MS = 100;

type BuildResult =
  | Readonly<{ kind: "succeeded" }>
  | Readonly<{ kind: "failed"; exitCode: number | null }>
  | Readonly<{ kind: "could-not-start"; error: Error }>;

function rustWasmWatcher(): Plugin {
  let debounceTimer: ReturnType<typeof setTimeout> | null = null;
  let activeBuild: Promise<void> | null = null;
  let activeProcess: ChildProcess | null = null;
  let rebuildQueued = false;

  function runBuild(): Promise<BuildResult> {
    return new Promise((complete) => {
      const process = spawn(BUILD_SCRIPT, [], {
        cwd: PROJECT_ROOT,
        stdio: "inherit",
      });
      let startError: Error | null = null;
      activeProcess = process;
      process.once("error", (error) => {
        startError = error;
      });
      process.once("close", (exitCode) => {
        activeProcess = null;
        if (startError !== null) {
          complete({ kind: "could-not-start", error: startError });
        } else if (exitCode === 0) {
          complete({ kind: "succeeded" });
        } else {
          complete({ kind: "failed", exitCode });
        }
      });
    });
  }

  async function drainBuildQueue(server: ViteDevServer): Promise<void> {
    do {
      rebuildQueued = false;
      server.config.logger.info("Rust changed. Rebuilding Brewthink WASM...");
      const result = await runBuild();

      switch (result.kind) {
        case "succeeded":
          server.config.logger.info("Brewthink WASM rebuilt. Vite will reload the browser.");
          break;
        case "failed":
          server.config.logger.error(
            `Rust/WASM build failed with exit code ${String(result.exitCode)}. Keeping the last good browser build.`,
          );
          break;
        case "could-not-start":
          server.config.logger.error(
            `Could not start the Rust/WASM build: ${result.error.message}`,
          );
          break;
        default: {
          const exhaustive: never = result;
          return exhaustive;
        }
      }
    } while (rebuildQueued);
  }

  function startBuild(server: ViteDevServer): void {
    if (activeBuild !== null) {
      rebuildQueued = true;
      return;
    }

    activeBuild = drainBuildQueue(server).finally(() => {
      activeBuild = null;
      if (rebuildQueued) {
        startBuild(server);
      }
    });
  }

  function requestBuild(server: ViteDevServer): void {
    if (debounceTimer !== null) {
      clearTimeout(debounceTimer);
    }
    debounceTimer = setTimeout(() => {
      debounceTimer = null;
      startBuild(server);
    }, DEBOUNCE_MS);
  }

  return {
    name: "brewthink-rust-wasm-watcher",
    apply: "serve",
    configureServer(server) {
      server.watcher.add(WATCHED_PATHS);
      const handleFileEvent = (eventName: string, changedPath: string): void => {
        if (REBUILD_EVENTS.has(eventName) && isRustBuildInput(changedPath)) {
          requestBuild(server);
        }
      };
      server.watcher.on("all", handleFileEvent);
      server.httpServer?.once("close", () => {
        server.watcher.off("all", handleFileEvent);
        if (debounceTimer !== null) {
          clearTimeout(debounceTimer);
        }
        activeProcess?.kill("SIGTERM");
      });
    },
  };
}

function isRustBuildInput(changedPath: string): boolean {
  const absolutePath = isAbsolute(changedPath) ? changedPath : resolve(PROJECT_ROOT, changedPath);
  const relativePath = relative(PROJECT_ROOT, absolutePath);
  return (
    WATCHED_FILES.has(relativePath) ||
    (relativePath.startsWith(`src${sep}`) && relativePath.endsWith(".rs"))
  );
}

export default defineConfig({
  plugins: [rustWasmWatcher()],
});
