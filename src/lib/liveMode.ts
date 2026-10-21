import type { BotConfig, BotDeskStatus, BotKind, LiveTerms, OpenPosition, TradeMode } from "@/lib/ipc/bot/bot";
import { ALL_KINDS, botMode, kindRunning } from "@/lib/ipc/bot/bot";
import type { StrategyBotView, StrategyConfig, StrategyLiveView } from "@/lib/ipc/strategy/strategy";
import { isActive } from "@/lib/ipc/strategy/strategy";

/**
 * Paper / Live per bot (0.3.19, one build). Pure helpers shared by the
 * creation screens, the header indicator and their tests. Rust re-checks
 * every condition; these only decide what the screen offers and why not.
 */

/** One reason Live cannot be chosen now: an i18n key and its params. */
export interface Blocker {
  key: string;
  params?: Record<string, string | number>;
}

/** What a Live choice depends on, read from the desk. */
export interface LiveReadiness {
  /** The bot trades futures on an exchange with a real-money order path. */
  futures: boolean;
  /** A verified trade-only key for `venue` is in the unlocked vault. */
  keyOk: boolean;
  /** `venue` passed its test-network run (desk status `liveVenues`). */
  venueOk: boolean;
  venueName: string;
  /** Risk settings or the store unreadable (desk status `tradingBlocked`). */
  tradingBlocked: string | null;
  /** Kind-specific reasons (a stop for DCA / Grid, paper positions, an unarmed bot). */
  extra?: Blocker[];
}

/** Why Live cannot be chosen, in the order the user fixes them; empty = it can. */
export function liveBlockers(r: LiveReadiness): Blocker[] {
  const out: Blocker[] = [];
  if (!r.futures) out.push({ key: "liveMode.blockers.futuresOnly" });
  if (r.tradingBlocked) out.push({ key: "liveMode.blockers.tradingBlocked" });
  if (r.futures && !r.venueOk) out.push({ key: "vault.venueNotDryRun", params: { exchange: r.venueName } });
  if (r.futures && !r.keyOk) out.push({ key: "liveMode.blockers.key", params: { exchange: r.venueName } });
  return [...out, ...(r.extra ?? [])];
}

/** A signal bot's book holds a paper position: it cannot take real money. */
export function holdsPaper(positions: OpenPosition[], kind: BotKind): boolean {
  return positions.some((p) => p.botKind === kind && !p.live);
}

/**
 * What Rust's `live_terms_changed` compares: the sizing the typed LIVE
 * confirmed, and for the TradingView bot its budget and max size per alert.
 */
export function liveTermsChanged(saved: BotConfig, next: BotConfig): boolean {
  return (
    saved.capital !== next.capital ||
    saved.leverage !== next.leverage ||
    saved.maxPositions !== next.maxPositions ||
    saved.exchangeId !== next.exchangeId ||
    saved.tv?.budgetUsdt !== next.tv?.budgetUsdt ||
    saved.tv?.maxSizeUsdt !== next.tv?.maxSizeUsdt
  );
}

/**
 * Live orders go to the Binance testnet, not to real money (Rust
 * `binanceIsProduction`, read once the first status arrived). The testnet
 * build then says "testnet" where production says "real money".
 */
export function ordersOnTestnet(s: BotDeskStatus, loaded: boolean): boolean {
  return loaded && s.binanceIsProduction === false;
}

/** The i18n key for a real-money label, or its testnet wording on the testnet. */
export function liveWordKey(key: "startLive" | "goLiveTitle" | "realScope", testnet: boolean): string {
  const keys = {
    startLive: ["liveMode.startLive", "liveMode.startTestnet"],
    goLiveTitle: ["liveMode.signal.goLiveTitle", "liveMode.signal.goLiveTitleTestnet"],
    realScope: ["pnl.scope.real", "pnl.scope.realTestnet"],
  } as const;
  return keys[key][testnet ? 1 : 0];
}

/** The label key of a statistics scope ("simulated" / "real"); real money on the testnet is "Testnet". */
export function scopeLabelKey(scope: string, testnet: boolean): string {
  return scope === "real" ? liveWordKey("realScope", testnet) : `pnl.scope.${scope}`;
}

/** The header chip's tooltip: what the count means, or how to get a Live bot running. */
export function liveChipTooltipKey(running: number): string {
  return running > 0 ? "liveMode.header.tooltip" : "liveMode.header.tooltipNone";
}

/**
 * What the signal bot form's main button does for the chosen mode:
 * "start" (configure + start), "confirmLive" (save, then the typed LIVE with
 * the money at risk, then start), or "toPaper" (a Live bot the user wants
 * back on paper).
 */
export function signalStartAction(saved: BotConfig | null, chosen: TradeMode, next: BotConfig | null): "start" | "confirmLive" | "toPaper" {
  const mode = botMode(saved);
  if (chosen === "paper") return mode === "live" ? "toPaper" : "start";
  if (mode !== "live" || saved?.live !== true) return "confirmLive";
  return next && saved && liveTermsChanged(saved, next) ? "confirmLive" : "start";
}

/** DCA / Grid: a level the exchange stop can rest at (Rust `has_protective_level`). */
export function hasProtectiveLevel(cfg: StrategyConfig): boolean {
  if ((cfg.maxDrawdownPct ?? 0) > 0) return true;
  return cfg.params.kind === "dca" ? (cfg.params.slPct ?? 0) > 0 : (cfg.params.stopOutPct ?? 0) > 0;
}

/** One Live bot as the header lists it. */
export interface LiveBotLine {
  id: string;
  /** i18n key of the bot's kind name, or the DCA / Grid bot's own name. */
  nameKey?: string;
  name?: string;
  running: boolean;
  /** A Live bot that needs its typed LIVE this session before it may start. */
  needsArm: boolean;
}

/** Every Live bot (signal and DCA / Grid), running first. */
export function liveBots(s: BotDeskStatus, views: StrategyLiveView[], bots: StrategyBotView[] | null): LiveBotLine[] {
  const signal: LiveBotLine[] = ALL_KINDS.filter((k) => botMode(s[k]) === "live").map((k) => ({
    id: k,
    nameKey: `bots.kind.${k}`,
    running: kindRunning(s, k),
    needsArm: s[k]?.live !== true,
  }));
  const strategy: LiveBotLine[] = views
    .filter((v) => v.enabled)
    .map((v) => {
      const bot = bots?.find((b) => b.id === v.botId);
      return { id: v.botId, name: bot?.name ?? v.botId, running: !!bot && isActive(bot), needsArm: v.armed !== true };
    });
  return [...signal, ...strategy].sort((a, b) => Number(b.running) - Number(a.running));
}

/** The header's count: Live bots taking new entries or cycles now. */
export function liveRunningCount(lines: LiveBotLine[]): number {
  return lines.filter((l) => l.running).length;
}

/** The margin a confirmation sends back (Rust re-checks it, `liveTermsChanged`). */
export function confirmedMargin(terms: LiveTerms): number {
  return terms.marginAtRiskUsdt;
}
