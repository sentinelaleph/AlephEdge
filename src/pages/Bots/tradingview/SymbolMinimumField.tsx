import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/Button/Button";
import { localeForLanguage } from "@/i18n";
import { formatNumber, formatPrice } from "@/lib/format";
import { tvSymbolMinimum, type TvSymbolMinimum } from "@/lib/ipc/tvlink/tvlink";
import { FieldShell } from "@/pages/Bots/strategy/formControls";

/**
 * The TradingView bot's Limits: type a pair, read the smallest order Binance
 * takes for it now (lot size and MIN_NOTIONAL at the live price, from Rust's
 * `tv_symbol_minimum`), and whether this bot's max size per alert reaches
 * it. An alert below it is refused with the same number.
 */
export function SymbolMinimumField({ maxSizeUsdt }: { maxSizeUsdt: number }) {
  const { t, i18n } = useTranslation();
  const locale = localeForLanguage(i18n.resolvedLanguage ?? "en");
  const [symbol, setSymbol] = useState("BTCUSDT");
  const [result, setResult] = useState<TvSymbolMinimum | null>(null);
  const [busy, setBusy] = useState(false);
  const k = (x: string) => `botsTv.symbolMin.${x}`;
  const usd = (v: number) => formatNumber(v, locale, { maximumFractionDigits: 2 });

  const check = async () => {
    setBusy(true);
    try {
      setResult(await tvSymbolMinimum(symbol));
    } catch {
      setResult({ symbol: symbol.trim().toUpperCase(), status: "unavailable", price: null, minQty: null, minNotional: null, minUsdt: null });
    } finally {
      setBusy(false);
    }
  };

  const line = (() => {
    if (!result) return null;
    if (result.status === "invalid") return <span className="ae-sfield__line ae-error">{t(k("invalid"))}</span>;
    if (result.status !== "ok" || result.minUsdt === null) return <span className="ae-sfield__line ae-error">{t(k("unavailable"))}</span>;
    const above = maxSizeUsdt > 0 && result.minUsdt > maxSizeUsdt + 1e-9;
    return (
      <>
        <span className="ae-sfield__line" role="status">
          {t(k("result"), {
            symbol: result.symbol,
            min: usd(result.minUsdt),
            qty: formatNumber(result.minQty ?? 0, locale, { maximumFractionDigits: 8 }),
            price: result.price !== null ? formatPrice(result.price, locale) : "",
          })}
        </span>
        {above ? (
          <span className="ae-sfield__line ae-error" role="alert">
            {t(k("aboveMax"), { symbol: result.symbol, min: usd(result.minUsdt), max: usd(maxSizeUsdt) })}
          </span>
        ) : null}
      </>
    );
  })();

  return (
    <FieldShell label={t(k("label"))} htmlFor="tvb-minsym" hint={<>{line ?? <span className="ae-sfield__line">{t(k("hint"))}</span>}</>}>
      <div className="ae-toolbar">
        <input
          id="tvb-minsym"
          className="ae-field__input mono"
          value={symbol}
          maxLength={30}
          onChange={(e) => setSymbol(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") {
              e.preventDefault();
              void check();
            }
          }}
        />
        <Button size="sm" variant="secondary" disabled={busy || !symbol.trim()} onClick={() => void check()}>
          {t(k("check"))}
        </Button>
      </div>
    </FieldShell>
  );
}
