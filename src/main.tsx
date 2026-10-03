import React from "react";
import ReactDOM from "react-dom/client";
import { MotionConfig } from "framer-motion";
import App from "./App";
import { ThemeProvider } from "./theme/ThemeProvider";
import "./i18n";
import "./styles/tokens.css";
import "./styles/global.css";

const root = ReactDOM.createRoot(document.getElementById("root") as HTMLElement);

function render(content: React.ReactNode) {
  root.render(
    <React.StrictMode>
      {/* reducedMotion="user" makes every framer-motion animation honor the OS
          prefers-reduced-motion setting (transforms, layout, springs) — the CSS
          token collapse only covers plain CSS transitions. */}
      <MotionConfig reducedMotion="user">
        <ThemeProvider>{content}</ThemeProvider>
      </MotionConfig>
    </React.StrictMode>,
  );
}

// Dev-only screenshot harness (`?shot=<page>`, see dev/shot/ShotHarness.tsx).
// `import.meta.env.DEV` is replaced with `false` at build time, so this branch
// and the dynamic import behind it are dropped from the production bundle.
const shot = import.meta.env.DEV ? new URLSearchParams(window.location.search).get("shot") : null;

if (import.meta.env.DEV && shot) {
  void import("./dev/shot/ShotHarness").then(({ ShotHarness, isShotName }) =>
    render(isShotName(shot) ? <ShotHarness shot={shot} /> : <App />),
  );
} else {
  render(<App />);
}
