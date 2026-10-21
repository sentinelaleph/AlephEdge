/**
 * The TradingView alert message builder: a form that produces the JSON the
 * Rust reader parses (src-tauri/src/tvlink/message.rs). Pure, so the
 * builder and its tests share one formula. Nothing here is stored: the
 * passphrase typed into the builder goes only into the copied text.
 */

export type BuilderMode = "manual" | "strategy";
export type ManualAction = "buy" | "sell" | "close";
export type LevelKind = "pct" | "price";
export type IdKind = "none" | "time" | "custom";

export interface BuilderForm {
  mode: BuilderMode;
  /** Manual mode only; strategy mode sends TradingView's order action. */
  action: ManualAction;
  market: "futures" | "spot";
  levels: LevelKind;
  /** Text as typed: a number, or a TradingView placeholder for a price. */
  tp: string;
  sl: string;
  /** Optional notional in USDT; empty = the bot's own maximum. */
  size: string;
  /** Add `"price":"{{close}}"` (the price-move guard's reference). */
  price: boolean;
  passphrase: string;
  id: IdKind;
  customId: string;
  /** Strategy mode: add `{{strategy.order.alert_message}}` (Pine's own levels). */
  alertMessage: boolean;
}

export const TICKER = "{{ticker}}";
export const CLOSE = "{{close}}";
export const TIMENOW = "{{timenow}}";
export const ORDER_ACTION = "{{strategy.order.action}}";
export const MARKET_POSITION = "{{strategy.market_position}}";
export const ALERT_MESSAGE = "{{strategy.order.alert_message}}";

export const DEFAULT_FORM: BuilderForm = {
  mode: "manual",
  action: "buy",
  market: "futures",
  levels: "pct",
  tp: "3",
  sl: "1.5",
  size: "",
  price: true,
  passphrase: "",
  id: "time",
  customId: "",
  alertMessage: false,
};

/** Why the message cannot be built (an i18n key under settings.tv.builder.error). */
export type BuildError = "levels" | "size" | "pct" | "spotShort" | "customId";

export interface Built {
  json: string | null;
  error: BuildError | null;
}

const NUMBER = /^\s*\d+(\.\d+)?\s*$/;
const PLACEHOLDER = /^\s*\{\{[a-z0-9_.]+\}\}\s*$/i;

/** A level as JSON: a number when numeric, a placeholder kept as text. */
function levelValue(text: string, kind: LevelKind): number | string | null {
  if (NUMBER.test(text)) {
    const n = Number(text);
    if (!(n > 0)) return null;
    if (kind === "pct" && n >= 100) return null;
    return n;
  }
  if (kind === "price" && PLACEHOLDER.test(text)) return text.trim();
  return null;
}

/** Builds the message, or says which field stops it. */
export function buildMessage(f: BuilderForm): Built {
  const fail = (error: BuildError): Built => ({ json: null, error });
  const strategy = f.mode === "strategy";
  const entry = strategy || f.action !== "close";
  if (!strategy && f.market === "spot" && f.action === "sell") return fail("spotShort");
  const msg: Record<string, string | number> = {};
  if (strategy) {
    msg.action = ORDER_ACTION;
    msg.market_position = MARKET_POSITION;
  } else {
    msg.action = f.action;
  }
  msg.symbol = TICKER;
  if (f.market === "spot") msg.bot = "spot";
  if (entry) {
    const tp = levelValue(f.tp, f.levels);
    const sl = levelValue(f.sl, f.levels);
    if (tp === null || sl === null) return fail(f.levels === "pct" && (NUMBER.test(f.tp) && NUMBER.test(f.sl)) ? "pct" : "levels");
    if (f.levels === "pct") {
      msg.tp_pct = tp;
      msg.sl_pct = sl;
    } else {
      msg.tp = tp;
      msg.sl = sl;
    }
    if (f.size.trim()) {
      if (!NUMBER.test(f.size) || !(Number(f.size) >= 5)) return fail("size");
      msg.size_usdt = Number(f.size);
    }
  }
  if (f.price) msg.price = CLOSE;
  if (f.passphrase.trim()) msg.passphrase = f.passphrase.trim();
  if (f.id === "time") msg.id = TIMENOW;
  if (f.id === "custom") {
    const id = f.customId.trim();
    if (!id || id.length > 64 || id.includes('"')) return fail("customId");
    msg.id = id;
  }
  let json = JSON.stringify(msg);
  // TradingView fills the placeholder with Pine's own JSON object: it goes in unquoted.
  if (strategy && f.alertMessage) json = `${json.slice(0, -1)},"alert_message":${ALERT_MESSAGE}}`;
  return { json, error: null };
}
