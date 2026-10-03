/**
 * DEV-ONLY screenshot harness. Reached from main.tsx only when
 * `import.meta.env.DEV` is true and the URL carries `?shot=<name>`, through a
 * dynamic import, so the production bundle contains neither this file nor its
 * sample data (verify: `npm run build`, then grep dist for "ae-shot").
 *
 * Each shot renders the REAL app shell on one route, fed by the sample desk
 * context in shot.mock.ts instead of DeskProvider. No login form is shown and
 * no credential is typed: the membership and vault gates are simply not
 * mounted. Every screen carries a "Sample data" label so a screenshot is never
 * mistaken for a real account or a track record.
 *
 *   ?shot=dashboard | bots | bot-new | bot-detail | signal-bots | dca | grid |
 *         dca-new | grid-new | dca-preset | dca-detail | grid-detail | dca-settings |
 *         signals | presets | preset | backtest |
 *         backtest-report | positions | positions-exchange | history |
 *         accounts (settings, exchange keys tab) | risk | settings | settings-devices | account | not-found
 *
 * The older names map onto pages: desk → signal-bots, feed → positions,
 * pnl → history, signals → signals, settings → the Futures bot's settings tab.
 */

import { useState } from "react";
import { AppShell } from "@/app/AppShell/AppShell";
import { DeskContextProvider } from "@/app/DeskProvider";
import { SAMPLE_DESK_CONTEXT } from "./shot.mock";
import "./ShotHarness.css";

const SHOT_ROUTES = {
  dashboard: "/dashboard",
  bots: "/bots",
  "bot-new": "/bots/new",
  "bot-detail": "/bots/signal-futures",
  "signal-bots": "/bots/signal",
  dca: "/bots/dca",
  grid: "/bots/grid",
  "dca-new": "/bots/dca/new",
  "grid-new": "/bots/grid/new",
  "dca-detail": "/bots/sb_sampledcabot",
  "grid-detail": "/bots/sb_samplegridbt",
  "dca-settings": "/bots/sb_sampledcabot?tab=settings",
  "dca-preset": "/bots/dca/new?preset=dca_long_classic",
  signals: "/signals?id=s1",
  presets: "/presets",
  preset: "/presets/dca_long_classic",
  backtest: "/backtest",
  guide: "/guide?s=dca",
  "backtest-report": "/backtest/sample",
  positions: "/positions",
  "positions-exchange": "/positions?tab=exchange",
  history: "/history",
  accounts: "/settings?tab=keys",
  risk: "/risk",
  settings: "/bots/signal-futures?tab=settings",
  "settings-page": "/settings",
  "settings-devices": "/settings?tab=devices",
  "settings-about": "/settings?tab=about",
  account: "/account",
  "not-found": "/no-such-page",
  // Older shot names, kept so existing screenshot scripts still resolve.
  desk: "/bots/signal",
  feed: "/positions",
  pnl: "/history",
} as const;

export type ShotName = keyof typeof SHOT_ROUTES;
export const SHOTS = Object.keys(SHOT_ROUTES) as ShotName[];

export function isShotName(v: string | null): v is ShotName {
  return v !== null && v in SHOT_ROUTES;
}

export function ShotHarness({ shot }: { shot: ShotName }) {
  // Put the shot's route in the hash once, before the shell reads it.
  useState(() => {
    window.history.replaceState({}, "", `${window.location.pathname}${window.location.search}#${SHOT_ROUTES[shot]}`);
    return null;
  });
  return (
    <>
      <div className="ae-shot" data-shot={shot}>
        <DeskContextProvider value={SAMPLE_DESK_CONTEXT}>
          <AppShell />
        </DeskContextProvider>
      </div>
      <p className="ae-shot__label">Sample data · simulated fills · not a trading record</p>
    </>
  );
}
