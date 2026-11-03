import { defineConfig, mergeConfig } from "vitest/config";
import { fileURLToPath, URL } from "node:url";
import viteConfig from "./vite.config";

const engine = (p: string) => fileURLToPath(new URL(`./pine-engine/src/lib/${p}`, import.meta.url));

// Reuses the app's Vite config (the "@" alias, the React plugin). Tests run in
// Node by default; a file that needs a DOM opts in with
// `// @vitest-environment jsdom` on its first line.
//
// The Pine-lite engine copy (pine-engine/, byte-identical to the website's
// engine) imports "@/lib/pine", "@/lib/ta" and "@/lib/indicators"; those
// three resolve to the copy. mergeConfig puts these aliases before the app's
// "@" alias, so they win.
export default mergeConfig(
  viteConfig,
  defineConfig({
    resolve: {
      alias: [
        { find: /^@\/lib\/pine/, replacement: engine("pine") },
        { find: /^@\/lib\/ta\//, replacement: engine("ta/") },
        { find: /^@\/lib\/indicators$/, replacement: engine("indicators.ts") },
      ],
    },
    test: {
      include: ["src/**/*.test.{ts,tsx}"],
      environment: "node",
      restoreMocks: true,
    },
  }),
);
