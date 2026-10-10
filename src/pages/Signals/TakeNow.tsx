import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import type { TFunction } from "i18next";
import { Link } from "@/app/router/router";
import { withQuery } from "@/app/router/routes";
import { Button } from "@/components/ui/Button/Button";
import { Chip } from "@/components/ui/Chip/Chip";
import { localeForLanguage } from "@/i18n";
import { formatNumber, formatPrice, formatSignedUsdt, formatUsdt } from "@/lib/format";
import { botPreviewTake, botTakeSignal, type BotKind, type TakePreview } from "@/lib/ipc/bot/bot";
import type { Signal } from "@/lib/ipc/signal/signal";
import { localizeError } from "@/lib/errorText";
import { localizeDetail } from "@/lib/skipNotes";
import { targetLabel } from "@/lib/takeProfit";
/**
 * The signal bots that take this signal's market and side: Futures takes
 * futures signals, Spot spot longs. A signal without a market (older
 * payloads) can reach either.
 */
export function signalBotsFor(s: Pick<Signal, "direction" | "market_type">): BotKind[] {
  const market = s.market_type?.trim().toLowerCase();
  if (market === "futures") return ["futures"];
  if (market === "spot") return s.direction === "long" ? ["spot"] : [];
  return s.direction === "long" ? ["futures", "spot"] : ["futures"];
}

/** The raw code of a rejected command (Tauri rejects with the Rust string). */
function rawCode(e: unknown): string {
  if (typeof e === "string" && e.trim()) return e;
  if (e instanceof Error && e.message) return e.message;
  return "storeReadFailed";
}

/**
 * The signal bots that may take this signal by hand: the ones whose market
 * and side it fits (`signalBotsFor`), plus Pump when it is configured and
 * the signal is a futures one (or carries no market).
 */
export function takeBotsFor(s: Pick<Signal, "direction" | "market_type">, pumpConfigured: boolean): BotKind[] {
  const bots = signalBotsFor(s);
  return pumpConfigured && bots.includes("futures") ? [...bots, "pump"] : bots;
}

/**
 * A refusal from `bot_take_signal` / `bot_preview_take` (`key` or
 * `key|detail`): the manual-entry wording first, then the skip reason the
 * bot feed uses, then the error table.
 */
export function takeRefusalText(t: TFunction, i18nExists: (key: string) => boolean, raw: string): string {
  const [head, ...rest] = raw.split("|");
  const code = head.trim();
  const detail = rest.join("|").trim();
  const own = `signalsPage.take.refusal.${code}`;
  if (i18nExists(own)) return t(own, { detail });
  const skip = `bots.skipReasons.${code}`;
  if (i18nExists(skip)) return t(skip, { detail: detail ? localizeDetail(detail) : "" });
  return localizeError(raw);
}

const RR_DIGITS = { minimumFractionDigits: 2, maximumFractionDigits: 2 };

type Row =
  | { state: "loading" }
  | { state: "refused"; code: string }
  | { state: "ready"; preview: TakePreview; busy: boolean }
  | { state: "opened"; entry: number };

function TakeRow({ kind, signal, onClose }: { kind: BotKind; signal: Signal; onClose: () => void }) {
  const { t, i18n } = useTranslation();
  const locale = localeForLanguage(i18n.resolvedLanguage ?? "en");
  const [row, setRow] = useState<Row>({ state: "loading" });

  useEffect(() => {
    let alive = true;
    setRow({ state: "loading" });
    botPreviewTake(kind, signal.id)
      .then((preview) => alive && setRow({ state: "ready", preview, busy: false }))
      .catch((e: unknown) => alive && setRow({ state: "refused", code: rawCode(e) }));
    return () => {
      alive = false;
    };
  }, [kind, signal.id]);

  const open = async (preview: TakePreview) => {
    setRow({ state: "ready", preview, busy: true });
    try {
      const pos = await botTakeSignal(kind, signal.id);
      setRow({ state: "opened", entry: pos.entry });
    } catch (e) {
      setRow({ state: "refused", code: rawCode(e) });
    }
  };

  const bot = t(`bots.kind.${kind}`);
  const botPath = withQuery("/bots/signal", { bot: kind === "futures" ? null : kind });
  return (
    <li className="ae-exec__take" data-bot={kind}>
      <span className="ae-exec__name">{bot}</span>
      {row.state === "loading" ? (
        <span className="ae-subtle">{t("signalsPage.take.pricing")}</span>
      ) : row.state === "refused" ? (
        <span className="ae-exec__refusal" role="status">
          {takeRefusalText(t, (k) => i18n.exists(k), row.code)}
        </span>
      ) : row.state === "opened" ? (
        <span className="ae-exec__opened" role="status">
          {t("signalsPage.take.opened", { symbol: signal.symbol, price: formatPrice(row.entry, locale) })}{" "}
          <Link to="/positions" className="ae-link" onClick={onClose}>
            {t("nav.positions")}
          </Link>{" "}
          <Link to={botPath} className="ae-link" onClick={onClose}>
            {t("signalsPage.take.botPage", { bot })}
          </Link>
        </span>
      ) : (
        <>
          <dl className="ae-exec__figures tabular">
            <div>
              <dt>{t("signalsPage.take.fill")}</dt>
              <dd>{formatPrice(row.preview.entry, locale)}</dd>
            </div>
            <div>
              <dt>{t("signalsPage.take.publishedEntry")}</dt>
              <dd>{formatPrice(row.preview.publishedEntry, locale)}</dd>
            </div>
            <div>
              <dt>{t("signalsPage.take.rr")}</dt>
              <dd>
                {t("signalsPage.take.rrValue", {
                  fill: formatNumber(row.preview.rrAtFill, locale, RR_DIGITS),
                  published: formatNumber(row.preview.rrPublished, locale, RR_DIGITS),
                })}
              </dd>
            </div>
            <div>
              <dt>{t("signalsPage.take.size")}</dt>
              <dd>
                {t("signalsPage.take.sizeValue", {
                  amount: formatUsdt(row.preview.notionalUsdt, locale, 2),
                  leverage: row.preview.effectiveLeverage.toFixed(2),
                })}
                {row.preview.riskCapped ? (
                  <>
                    {" "}
                    <Chip tone="warn" title={t("bots.position.riskCappedTooltip")}>
                      {t("bots.position.riskCappedBadge")}
                    </Chip>
                  </>
                ) : null}
              </dd>
            </div>
            <div>
              <dt>{t("signalsPage.take.lossAtStop", { price: formatPrice(row.preview.stop, locale) })}</dt>
              <dd data-tone="down">{formatSignedUsdt(row.preview.lossAtStopUsdt, locale)}</dd>
            </div>
            <div>
              <dt>
                {t("signalsPage.take.gainAtTarget", {
                  target: targetLabel(row.preview.tpTarget, t("bots.tp.custom")),
                  price: formatPrice(row.preview.target, locale),
                })}
              </dt>
              <dd data-tone="up">{formatSignedUsdt(row.preview.gainAtTargetUsdt, locale)}</dd>
            </div>
            <div>
              <dt>{t("signalsPage.take.fees")}</dt>
              <dd>{formatUsdt(row.preview.feesUsdt, locale, 2)}</dd>
            </div>
          </dl>
          {row.preview.rrLow ? (
            <span className="ae-exec__warn" role="note">
              {t("signalsPage.take.rrLow")}
            </span>
          ) : null}
          <Button variant="primary" size="xs" disabled={row.busy} onClick={() => void open(row.preview)}>
            {row.busy ? t("signalsPage.take.opening") : t("signalsPage.take.open")}
          </Button>
        </>
      )}
    </li>
  );
}

/**
 * "Take this signal now · paper": opens the signal on a fitting signal bot's
 * paper book (bot_take_signal). The bot's filters are skipped, its risk
 * limits are not; a bot on real money refuses.
 */
export function TakeNow({ signal, pumpConfigured, onClose }: { signal: Signal; pumpConfigured: boolean; onClose: () => void }) {
  const { t } = useTranslation();
  const bots = takeBotsFor(signal, pumpConfigured);
  const title = t("signalsPage.take.title");
  return (
    <section className="ae-exec__section" aria-label={title}>
      <h3 className="ae-section-title">{title}</h3>
      <p className="ae-subtle">{t("signalsPage.take.line")}</p>
      {bots.length === 0 ? (
        <p className="ae-subtle">{t("signalsPage.take.noBot")}</p>
      ) : (
        <ul className="ae-exec__list">
          {bots.map((k) => (
            <TakeRow key={k} kind={k} signal={signal} onClose={onClose} />
          ))}
        </ul>
      )}
    </section>
  );
}
