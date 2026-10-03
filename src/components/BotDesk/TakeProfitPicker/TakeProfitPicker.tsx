import { useEffect, useId, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import type { TakeProfitTarget } from "@/lib/ipc/bot/bot";
import { exchangeSymbols } from "@/lib/ipc/exchange/exchange";
import { signalRecent, type Signal } from "@/lib/ipc/signal/signal";
import { localeForLanguage } from "@/i18n";
import {
  AVERAGE_TP_DISTANCE_DATE,
  AVERAGE_TP_DISTANCE_PCT,
  customPctValid,
  MAX_CUSTOM_TP_PCT,
  MIN_CUSTOM_TP_PCT,
  signalTpDistances,
  TARGET_KINDS,
  type TargetKind,
} from "@/lib/takeProfit";
import { Button, IconButton } from "@/components/ui/Button/Button";
import { SegmentedControl } from "@/components/ui/SegmentedControl/SegmentedControl";
import { NO_VALUE } from "@/lib/format";
import "./TakeProfitPicker.css";
import { formatDecimalInput, parseDecimal } from "@/lib/decimal";

/** A target as the form holds it: the % stays text so typing is not fought. */
export interface TargetDraft {
  kind: TargetKind;
  pct: string;
}

export interface OverrideDraft extends TargetDraft {
  symbol: string;
}

export function draftFromTarget(t: TakeProfitTarget | undefined): TargetDraft {
  if (!t) return { kind: "tp1", pct: "" };
  return t.kind === "custom" ? { kind: "custom", pct: formatDecimalInput(t.pct) } : { kind: t.kind, pct: "" };
}

/** Sent as typed: an out-of-range % is refused by the server, not corrected. */
export function targetFromDraft(d: TargetDraft): TakeProfitTarget {
  return d.kind === "custom" ? { kind: "custom", pct: parseDecimal(d.pct) } : { kind: d.kind };
}

export function draftValid(d: TargetDraft): boolean {
  return d.kind !== "custom" || (d.pct !== "" && customPctValid(parseDecimal(d.pct)));
}

interface TakeProfitPickerProps {
  value: TargetDraft;
  onChange: (v: TargetDraft) => void;
  overrides: OverrideDraft[];
  onOverridesChange: (v: OverrideDraft[]) => void;
  /** The bot's symbol whitelist (uppercase); empty = every symbol. */
  botSymbols: string[];
  /** Where the bot trades: the per-coin list is that market's symbols. */
  exchangeId: string;
  futures: boolean;
  disabled?: boolean;
  /** False when a Panel titled "Take profit" already names the choice. */
  showLabel?: boolean;
}

/** How many matches the per-coin suggestion list shows at once. */
const MAX_SUGGESTIONS = 8;
/** Offered on an empty query when no recent signal names a coin. */
const MAJORS = ["BTCUSDT", "ETHUSDT", "SOLUSDT", "BNBUSDT", "XRPUSDT", "DOGEUSDT", "ADAUSDT", "AVAXUSDT"];

const SYMBOL_RE = /^[A-Z0-9]{2,20}$/;

/**
 * Which take-profit the bot exits at: TP1/TP2/TP3 of the signal or a custom
 * % from the fill, bot-wide plus per-coin overrides. Every option carries its
 * distance, measured on the latest signal when one is at hand, else the
 * 14-day average.
 */
export function TakeProfitPicker({
  value,
  onChange,
  overrides,
  onOverridesChange,
  botSymbols,
  exchangeId,
  futures,
  disabled,
  showLabel = true,
}: TakeProfitPickerProps) {
  const { t, i18n } = useTranslation();
  const locale = localeForLanguage(i18n.language);
  const signals = useRecentSignals();
  const listed = useExchangeSymbols(exchangeId, futures);
  const [newSymbol, setNewSymbol] = useState("");
  const [suggestOpen, setSuggestOpen] = useState(false);
  const listId = useId();

  const customRange = {
    min: MIN_CUSTOM_TP_PCT.toLocaleString(locale),
    max: MAX_CUSTOM_TP_PCT.toLocaleString(locale),
  };
  const pctFmt = (v: number) =>
    `+${v.toLocaleString(locale, { minimumFractionDigits: 1, maximumFractionDigits: 1 })}%`;

  // Newest first from the feed; within the bot's whitelist when it has one.
  const latestFor = (symbol?: string): Signal | undefined =>
    signals.find(
      (s) =>
        (symbol ? s.symbol === symbol : botSymbols.length === 0 || botSymbols.includes(s.symbol)) &&
        signalTpDistances(s).length > 0,
    );
  const latest = latestFor();
  const distances = latest ? signalTpDistances(latest) : [...AVERAGE_TP_DISTANCE_PCT];

  const customInvalid = value.kind === "custom" && !draftValid(value);

  const knownSymbols = useMemo(() => new Set(signals.map((s) => s.symbol)), [signals]);
  const typed = newSymbol.trim().toUpperCase();
  // Choices: the whitelist when the bot has one, else the exchange's symbols
  // plus any coin a recent signal named. Recent-signal coins lead an empty query.
  const choices = useMemo(() => {
    if (botSymbols.length > 0) return [...botSymbols].sort();
    return [...new Set([...listed, ...knownSymbols])].sort();
  }, [botSymbols, listed, knownSymbols]);
  // "SOL" means SOLUSDT when the market lists SOLUSDT and not SOL itself.
  const candidate =
    typed !== "" && !choices.includes(typed) && choices.includes(`${typed}USDT`) ? `${typed}USDT` : typed;
  const symbolError = (() => {
    if (candidate === "") return null;
    if (!SYMBOL_RE.test(candidate)) return t("bots.tp.errFormat");
    if (overrides.some((o) => o.symbol === candidate)) return t("bots.tp.errDuplicate");
    if (botSymbols.length > 0) {
      return botSymbols.includes(candidate) ? null : t("bots.tp.errNotInWhitelist");
    }
    // A coin with no open signal is still a valid target; one the exchange
    // does not trade is not. Without a list (fetch failed) only the format counts.
    if (listed.length > 0 && !listed.includes(candidate)) return t("bots.tp.errNotListed");
    return null;
  })();

  const taken = new Set(overrides.map((o) => o.symbol));
  const suggestions = (() => {
    const free = choices.filter((s) => !taken.has(s));
    if (typed === "") {
      const recent = free.filter((s) => knownSymbols.has(s));
      const majors = MAJORS.filter((s) => free.includes(s));
      return (recent.length > 0 ? recent : majors.length > 0 ? majors : free).slice(0, MAX_SUGGESTIONS);
    }
    const starts = free.filter((s) => s.startsWith(typed));
    const contains = free.filter((s) => !s.startsWith(typed) && s.includes(typed));
    return [...starts, ...contains].slice(0, MAX_SUGGESTIONS);
  })();

  const showSuggest = suggestOpen && !disabled && suggestions.length > 0;

  function addSymbol(symbol: string) {
    onOverridesChange([...overrides, { symbol, kind: value.kind, pct: value.pct }]);
    setNewSymbol("");
    setSuggestOpen(false);
  }

  function addOverride() {
    if (candidate === "" || symbolError) return;
    addSymbol(candidate);
  }

  function updateOverride(i: number, patch: Partial<OverrideDraft>) {
    onOverridesChange(overrides.map((o, j) => (j === i ? { ...o, ...patch } : o)));
  }

  /** "TP3 · +17.0%", or "Custom" for the custom option. */
  const optionLabel = (kind: TargetKind, dist: number[]) => {
    if (kind === "custom") return t("bots.tp.custom");
    const d = dist[Number(kind.slice(2)) - 1];
    return d != null ? `${kind.toUpperCase()} · ${pctFmt(d)}` : kind.toUpperCase();
  };

  const customPct = value.kind === "custom" && value.pct !== "" && customPctValid(parseDecimal(value.pct)) ? parseDecimal(value.pct) : null;

  return (
    <div className="ae-tp">
      <div className="ae-sfield ae-tp__wide">
        {showLabel ? <span className="ae-field__label">{t("bots.tp.title")}</span> : null}
        <SegmentedControl
          label={t("bots.tp.title")}
          value={value.kind}
          fill
          wrap
          disabled={disabled}
          onChange={(kind) => onChange({ ...value, kind })}
          options={TARGET_KINDS.map((kind) => {
            const idx = kind === "custom" ? -1 : Number(kind.slice(2)) - 1;
            const d = idx >= 0 ? distances[idx] : undefined;
            return {
              value: kind,
              label: kind === "custom" ? t("bots.tp.custom") : kind.toUpperCase(),
              sub: kind === "custom" ? (customPct !== null ? pctFmt(customPct) : "%") : d != null ? pctFmt(d) : NO_VALUE,
            };
          })}
        />
        <p className="ae-field__hint">
          {latest
            ? t("bots.tp.sourceSignal", { symbol: latest.symbol })
            : t("bots.tp.sourceAverage", {
                date: AVERAGE_TP_DISTANCE_DATE.toLocaleDateString(locale, { dateStyle: "medium", timeZone: "UTC" }),
              })}
        </p>
        {value.kind !== "tp1" ? <p className="ae-field__hint ae-warntext">{t("bots.tp.fartherWarning")}</p> : null}
      </div>

      {value.kind === "custom" ? (
        <div className="ae-sfield" data-invalid={customInvalid || undefined}>
          <label className="ae-field__label" htmlFor={`${listId}-custom`}>
            {t("bots.tp.customPct")}
          </label>
          <span className="ae-sfield__input">
            <input
              id={`${listId}-custom`}
              className="ae-field__input mono"
              type="text"
              inputMode="decimal"
              placeholder="40"
              value={value.pct}
              onChange={(e) => onChange({ ...value, pct: e.target.value })}
              aria-invalid={customInvalid || undefined}
              disabled={disabled}
            />
            <span className="ae-sfield__unit">%</span>
          </span>
          {customInvalid ? <p className="ae-sfield__error">{t("bots.tp.customOutOfRange", customRange)}</p> : null}
        </div>
      ) : null}

      <div className="ae-sfield ae-tp__wide">
        <span className="ae-field__label">
          {t("bots.tp.overrides")}
          {overrides.length > 0 ? <span className="ae-tp__count tabular">{overrides.length}</span> : null}
        </span>
        {overrides.length > 0 ? (
          <ul className="ae-tp__list">
            {overrides.map((o, i) => {
              const dist = (() => {
                const s = latestFor(o.symbol);
                return s ? signalTpDistances(s) : [...AVERAGE_TP_DISTANCE_PCT];
              })();
              const invalid = !draftValid(o);
              return (
                <li key={o.symbol} className="ae-tp__row" data-invalid={invalid || undefined}>
                  <span className="ae-tp__symbol mono">{o.symbol}</span>
                  <select
                    className="ae-field__input ae-tp__select"
                    value={o.kind}
                    aria-label={`${o.symbol} ${t("bots.tp.title")}`}
                    onChange={(e) => updateOverride(i, { kind: e.target.value as TargetKind })}
                    disabled={disabled}
                  >
                    {TARGET_KINDS.map((kind) => (
                      <option key={kind} value={kind}>
                        {optionLabel(kind, dist)}
                      </option>
                    ))}
                  </select>
                  {o.kind === "custom" ? (
                    <span className="ae-sfield__input ae-tp__pct">
                      <input
                        className="ae-field__input mono"
                        type="text"
                        inputMode="decimal"
                        placeholder="40"
                        aria-label={`${o.symbol} ${t("bots.tp.customPct")}`}
                        aria-invalid={invalid || undefined}
                        value={o.pct}
                        onChange={(e) => updateOverride(i, { pct: e.target.value })}
                        disabled={disabled}
                      />
                      <span className="ae-sfield__unit">%</span>
                    </span>
                  ) : null}
                  <IconButton
                    label={t("bots.tp.remove", { symbol: o.symbol })}
                    icon={<span aria-hidden="true">×</span>}
                    onClick={() => onOverridesChange(overrides.filter((_, j) => j !== i))}
                    disabled={disabled}
                  />
                  {invalid ? <p className="ae-sfield__error ae-tp__rowerror">{t("bots.tp.customOutOfRange", customRange)}</p> : null}
                </li>
              );
            })}
          </ul>
        ) : null}
        <div className="ae-tp__add">
          <input
            className="ae-field__input mono"
            placeholder={t("bots.tp.searchPlaceholder")}
            aria-label={t("bots.tp.symbol")}
            aria-invalid={(symbolError != null && !showSuggest) || undefined}
            value={newSymbol}
            onChange={(e) => {
              setNewSymbol(e.target.value.toUpperCase());
              setSuggestOpen(true);
            }}
            onFocus={() => setSuggestOpen(true)}
            onBlur={() => setSuggestOpen(false)}
            onKeyDown={(e) => {
              if (e.key === "Enter") {
                e.preventDefault();
                addOverride();
              } else if (e.key === "Escape") {
                setSuggestOpen(false);
              }
            }}
            role="combobox"
            aria-expanded={showSuggest}
            aria-controls={listId}
            aria-autocomplete="list"
            autoComplete="off"
            disabled={disabled}
          />
          <Button variant="secondary" size="sm" onClick={addOverride} disabled={disabled || candidate === "" || symbolError != null}>
            {t("bots.tp.add")}
          </Button>
        </div>
        {showSuggest ? (
          <ul className="ae-tp__suggest" id={listId} role="listbox" aria-label={t("bots.tp.symbol")}>
            {suggestions.map((s) => (
              <li key={s} role="option" aria-selected={false}>
                <button
                  type="button"
                  className="ae-tp__suggestBtn mono"
                  // mousedown, not click: the input's blur would close the list first.
                  onMouseDown={(e) => {
                    e.preventDefault();
                    addSymbol(s);
                  }}
                >
                  {s}
                  {knownSymbols.has(s) ? <span className="ae-tp__suggestTag">{t("bots.tp.hasSignal")}</span> : null}
                </button>
              </li>
            ))}
          </ul>
        ) : null}
        {/* While matches are on screen the query is still being typed. */}
        {symbolError && !showSuggest ? <p className="ae-sfield__error">{symbolError}</p> : null}
      </div>
    </div>
  );
}

/** The recent signal feed, refreshed every minute; empty when unavailable. */
/** The exchange's tradable symbols for the bot's market; [] until loaded. */
function useExchangeSymbols(exchangeId: string, futures: boolean): string[] {
  const [symbols, setSymbols] = useState<string[]>([]);
  useEffect(() => {
    let alive = true;
    setSymbols([]);
    if (!exchangeId) return;
    exchangeSymbols(exchangeId, futures)
      .then((rows) => {
        if (alive) setSymbols(rows);
      })
      .catch(() => undefined);
    return () => {
      alive = false;
    };
  }, [exchangeId, futures]);
  return symbols;
}

function useRecentSignals(): Signal[] {
  const [signals, setSignals] = useState<Signal[]>([]);
  useEffect(() => {
    let alive = true;
    const load = () =>
      signalRecent()
        .then((rows) => {
          if (alive) setSignals(rows);
        })
        .catch(() => undefined);
    void load();
    const id = window.setInterval(load, 60_000);
    return () => {
      alive = false;
      window.clearInterval(id);
    };
  }, []);
  return signals;
}
