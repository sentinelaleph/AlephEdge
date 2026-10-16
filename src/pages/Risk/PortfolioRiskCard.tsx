import { useTranslation } from "react-i18next";
import { useDeskContext } from "@/app/DeskProvider";
import { Chip } from "@/components/ui/Chip/Chip";
import { FactList, Panel, type Fact } from "@/components/ui/Panel/Panel";
import { localeForLanguage } from "@/i18n";
import { formatNumber, formatPnl, NO_VALUE } from "@/lib/format";
import { activeBooks, scenarioAt } from "@/lib/riskView";
import { CheckChip } from "./PortfolioRisk";

/**
 * Right panel: the portfolio stress at BTC -20% per book, concentration in
 * one line each, and the threshold chips. Rust's figures, as served.
 */
export function PortfolioRiskCard() {
  const { t, i18n } = useTranslation();
  const locale = localeForLanguage(i18n.resolvedLanguage ?? "en");
  const { riskView } = useDeskContext();
  const s = riskView.snapshot;
  const pct = (v: number | null | undefined, digits = 0) =>
    v === null || v === undefined ? NO_VALUE : `${formatNumber(v, locale, { maximumFractionDigits: digits })}%`;

  if (!s) {
    return (
      <Panel title={t("safety.section.portfolio")}>
        <p className="ae-subtle">{riskView.error ? t("riskview.failed") : t("workspace.loading")}</p>
      </Panel>
    );
  }
  const books = activeBooks(s);
  const rows: Fact[] = [];
  for (const id of books) {
    const b = s[id];
    const sc = scenarioAt(b, -20);
    const loss = sc && sc.book.included > 0 ? sc.book : null;
    rows.push({
      label: t("riskview.card.stress", { book: t(`riskview.book.${id}`) }),
      value: loss
        ? `${formatPnl(loss.pnl, locale).text} · ${loss.pctOfCapital === null ? NO_VALUE : formatPnl(loss.pctOfCapital, locale, { unit: "percent", digits: 1 }).text}`
        : NO_VALUE,
      tone: loss && loss.pnl < 0 ? "down" : undefined,
    });
  }
  const main = books.includes("real") ? s.real : s.paper;
  if (books.length > 0) {
    const c = main.concentration;
    rows.push(
      { label: t("riskview.sameDirection"), value: c.sameDirectionPct === null ? NO_VALUE : `${pct(c.sameDirectionPct)} ${c.dominant ? t(`riskview.dir.${c.dominant}`) : ""}` },
      { label: t("riskview.topCoin"), value: c.topCoin ? `${c.topCoin} · ${pct(c.topCoinPct)}` : NO_VALUE },
      { label: t("riskview.avgBeta"), value: c.avgBeta === null ? NO_VALUE : formatNumber(c.avgBeta, locale, { minimumFractionDigits: 2, maximumFractionDigits: 2 }) },
    );
  }
  return (
    <Panel title={t("safety.section.portfolio")} aside={s.sample ? <Chip tone="accent">{t("riskview.sample")}</Chip> : null}>
      {books.length === 0 ? (
        <p className="ae-subtle">{t("riskview.empty")}</p>
      ) : (
        <>
          <FactList rows={rows} />
          {main.checks.length > 0 ? (
            <div className="ae-toolbar ae-riskview-checks" aria-label={t("riskview.checks")}>
              {main.checks.map((c) => (
                <CheckChip key={c.key} check={c} />
              ))}
            </div>
          ) : null}
        </>
      )}
    </Panel>
  );
}
