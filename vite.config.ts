import { defineConfig, type Plugin } from "vite";
import react from "@vitejs/plugin-react";
import { fileURLToPath, URL } from "node:url";

// Tauri expects a fixed dev port and does not want Vite to obscure Rust errors.
const host = process.env.TAURI_DEV_HOST;

/** Dev-only sources: `*.mock.ts(x)` browser fallbacks and the `src/dev/` harness. */
const DEV_ONLY_MODULE = /\.mock\.tsx?$|[\\/]src[\\/]dev[\\/]/;

/**
 * Must stay in sync with scripts/check-no-dev-mocks.cjs, which fails the build
 * when an emitted file contains it.
 */
const DEV_MOCK_MARKER = "__ALEPH_DEV_MOCK__";

/**
 * Keeps dev mocks out of the release bundle (`vite build` only; dev and vitest
 * are untouched).
 *
 * `devMock(() => import("./x.mock"), ...)` refuses at runtime outside dev, but
 * the bundler cannot see through that call, so each mock used to be emitted as
 * its own unused chunk. Here the dynamic import itself is replaced by a
 * rejection before bundling, so no chunk is produced. A static import of a mock
 * from app code fails the build. Mock modules get a side-effect marker, so if
 * one ever does reach a chunk the post-build scan finds it.
 */
function stripDevMocks(): Plugin {
  const dynamicMockImport = /\bimport\(\s*(["'])([^"']+\.mock)\1\s*\)/g;
  const staticMockImport = /\b(?:from|import)\s*(["'])([^"']+\.mock)\1/;
  return {
    name: "aleph:strip-dev-mocks",
    apply: "build",
    transform(code, id) {
      const file = id.split("?")[0];
      if (file.includes("node_modules") || !/\.[cm]?[jt]sx?$/.test(file)) return null;
      if (DEV_ONLY_MODULE.test(file)) {
        return { code: `globalThis.${DEV_MOCK_MARKER} = true;\n${code}`, map: null };
      }
      const out = code.replace(dynamicMockImport, 'Promise.reject(new Error("Dev mock is not part of this build."))');
      const leak = staticMockImport.exec(out);
      if (leak) this.error(`${file} imports dev mock "${leak[2]}" statically; use devMock(() => import(...)).`);
      return out === code ? null : { code: out, map: null };
    },
  };
}

export default defineConfig({
  plugins: [react(), stripDevMocks()],
  resolve: {
    alias: {
      "@": fileURLToPath(new URL("./src", import.meta.url)),
    },
  },
  // Prevent Vite from clearing the screen so Rust build errors stay visible.
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host
      ? { protocol: "ws", host, port: 1421 }
      : undefined,
    watch: {
      // Rust trees are not frontend source. Watching them also crashes the dev
      // server with EBUSY on Windows when a running runner .exe under
      // crates/*/target is locked.
      ignored: ["**/src-tauri/**", "**/crates/**", "**/target/**"],
    },
  },
  // Tauri uses a modern webview; target its supported engines.
  build: {
    target: process.env.TAURI_ENV_PLATFORM === "windows" ? "chrome105" : "safari13",
    minify: !process.env.TAURI_ENV_DEBUG,
    sourcemap: !!process.env.TAURI_ENV_DEBUG,
  },
});
