import { defineConfig, mergeConfig } from "vitest/config";
import viteConfig from "./vite.config";

// Reuses the app's Vite config (the "@" alias, the React plugin). Tests run in
// Node by default; a file that needs a DOM opts in with
// `// @vitest-environment jsdom` on its first line.
export default mergeConfig(
  viteConfig,
  defineConfig({
    test: {
      include: ["src/**/*.test.{ts,tsx}"],
      environment: "node",
      restoreMocks: true,
    },
  }),
);
