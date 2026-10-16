import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { useDeskContext } from "@/app/DeskProvider";
import { Button } from "@/components/ui/Button/Button";
import { Chip, LiveChip, type ChipTone } from "@/components/ui/Chip/Chip";
import { DataTable, type DataColumn } from "@/components/ui/DataTable/DataTable";
import { EmptyState } from "@/components/ui/EmptyState/EmptyState";
import { FactList, Panel } from "@/components/ui/Panel/Panel";
import { Section } from "@/components/ui/Section/Section";
import { SegmentedControl } from "@/components/ui/SegmentedControl/SegmentedControl";
import { TextField } from "@/components/ui/TextField/TextField";
import { localeForLanguage } from "@/i18n";
import { formatNumber, formatPnl, formatPrice, formatTableTime, formatUsdt, NO_VALUE, pnlToneAttr } from "@/lib/format";
import type { Check, CheckLevel, Contribution, ExposureRow, Scenario } from "@/lib/ipc/riskview/riskview";
import type { ViewThresholds } from "@/lib/ipc/risk/risk";
import { activeBooks, type BookId } from "@/lib/riskView";
import { riskviewErrorText } from "@/lib/riskViewText";

export const LEVEL_TONE: Record<CheckLevel, ChipTone> = { ok: "success", warn: "warn", danger: "danger", unavailable: "neutral" };

/** "BTC -20%" / "BTC +10%". */
export function moveLabel(move: number, locale: string): string {
  return `BTC ${move > 0 ? "+" : ""}${formatNumber(move, locale, { maximumFractionDigits: 0 })}%`;
}

const pctText = (v: number | null, locale: string, digits = 1) =>
  v === null ? NO_VALUE : `${formatNumber(v, locale, { maximumFractionDigits: digits })}%`;

/** One threshold check as a chip: name, value, level. */
export function CheckChip({ check }: { check: Check }) {
  const { t, i18n } = useTranslation();
  const locale = localeForLanguage(i18n.resolvedLanguage ?? "en");
  const value = check.value === null ? t("riskview.level.unavailable") : pctText(check.value, locale, 0);
  return (
    <Chip
      tone={LEVEL_TONE[check.level]}
      title={t("riskview.checkTitle", { level: t(`riskview.level.${check.level}`), threshold: check.threshold })}
    >
      {t(`riskview.check.${check.key}`)} {value}
    </Chip>
  );
}

/**
 * Portfolio stress (center): every open exposure under BTC shocks, the
 * largest contributors, concentration and the threshold checks, then the
 * exposures with their beta. All figures are Rust's (riskview_get).
 */
export function PortfolioRisk() {
  const { t, i18n } = useTranslation();
  const locale = localeForLanguage(i18n.resolvedLanguage ?? "en");
  const { riskView } = useDeskContext();
  const s = riskView.snapshot;
  const books = s ? activeBooks(s) : [];
  const [bookSel, setBookSel] = useState<BookId>("paper");
  const bookId: BookId = books.includes(bookSel) ? bookSel : (books[0] ?? "paper");
  const [move, setMove] = useState(-20);
  const book = s ? s[bookId] : null;
  const scenario = book?.scenarios.find((x) => x.btcMovePct === move) ?? null;
  const usdtSigned = (v: number) => formatPnl(v, locale).text;

  const scenarioColumns: DataColumn<Scenario>[] = [
    { id: "move", header: t("riskview.col.move"), cell: (r) => moveLabel(r.btcMovePct, locale) },
    {
      id: "result",
      header: t("riskview.col.result"),
      numeric: true,
      keep: true,
      cell: (r) => (r.book.included === 0 ? NO_VALUE : usdtSigned(r.book.pnl)),
      tone: (r) => (r.book.included === 0 ? undefined : pnlToneAttr(r.book.pnl)),
    },
    {
      id: "pct",
      header: t("riskview.col.pctCapital"),
      numeric: true,
      cell: (r) => (r.book.pctOfCapital === null ? NO_VALUE : formatPnl(r.book.pctOfCapital, locale, { unit: "percent", digits: 1 }).text),
      tone: (r) => pnlToneAttr(r.book.pctOfCapital),
    },
    {
      id: "share",
      header: t("riskview.col.btcShare"),
      headerTip: t("riskview.btcShareTip"),
      numeric: true,
      priority: 2,
      cell: (r) => pctText(r.book.btcSharePct, locale, 0),
    },
    { id: "excluded", header: t("riskview.col.excluded"), numeric: true, priority: 3, cell: (r) => r.book.excluded },
  ];

  const contribColumns: DataColumn<Contribution>[] = [
    { id: "symbol", header: t("riskview.col.symbol"), cell: (r) => r.symbol },
    { id: "bot", header: t("riskview.col.bot"), priority: 2, cell: (r) => sourceLabel(t, r.source, r.label) },
    {
      id: "pnl",
      header: t("riskview.col.result"),
      numeric: true,
      keep: true,
      cell: (r) => usdtSigned(r.pnl),
      tone: (r) => pnlToneAttr(r.pnl),
    },
    { id: "exit", header: t("riskview.col.exit"), priority: 2, cell: (r) => t(`riskview.exit.${r.exit ?? "open"}`, { defaultValue: r.exit ?? "" }) },
  ];

  const exposureColumns: DataColumn<ExposureRow>[] = [
    {
      id: "symbol",
      header: t("riskview.col.symbol"),
      cell: (r) => (
        <span className="ae-namecell">
          <span aria-hidden="true" className="ae-subtle">
            {r.direction === "short" ? "▼" : r.direction === "long" ? "▲" : "◆"}
          </span>
          {r.symbol}
          {r.live ? <LiveChip /> : null}
        </span>
      ),
    },
    { id: "bot", header: t("riskview.col.bot"), priority: 2, cell: (r) => sourceLabel(t, r.source, r.label) },
    { id: "capital", header: t("riskview.col.capital"), numeric: true, cell: (r) => formatUsdt(r.capital, locale) },
    { id: "mark", header: t("riskview.col.mark"), numeric: true, priority: 3, cell: (r) => (r.mark === null ? t("riskview.noPrice") : formatPrice(r.mark, locale)) },
    { id: "stop", header: t("riskview.col.stop"), numeric: true, priority: 2, cell: (r) => (r.stop === null ? NO_VALUE : formatPrice(r.stop, locale)) },
    { id: "liq", header: t("riskview.col.liq"), numeric: true, priority: 3, cell: (r) => (r.liq === null ? NO_VALUE : formatPrice(r.liq, locale)) },
    {
      id: "beta",
      header: t("riskview.col.beta"),
      numeric: true,
      keep: true,
      cell: (r) =>
        r.beta ? (
          <span title={r.beta.clamped ? t("riskview.betaClamped", { raw: formatNumber(r.beta.raw, locale, { maximumFractionDigits: 2 }) }) : undefined}>
            {formatNumber(r.beta.beta, locale, { minimumFractionDigits: 2, maximumFractionDigits: 2 })}
            {r.beta.clamped ? "*" : ""}
          </span>
        ) : (
          <span className="ae-subtle" title={r.betaError ? riskviewErrorText(t, r.betaError) : undefined}>
            {t("riskview.level.unavailable")}
          </span>
        ),
    },
    {
      id: "r2",
      header: t("riskview.col.r2"),
      headerTip: t("riskview.btcShareTip"),
      numeric: true,
      priority: 3,
      cell: (r) => (r.beta ? formatNumber(r.beta.r2, locale, { minimumFractionDigits: 2, maximumFractionDigits: 2 }) : NO_VALUE),
    },
  ];

  const c = book?.concentration;
  return (
    <Section
      title={t("safety.section.portfolio")}
      aside={
        <span className="ae-toolbar">
          {s?.sample ? <Chip tone="accent">{t("riskview.sample")}</Chip> : null}
          {s ? <span className="ae-subtle">{t("riskview.computedAt", { time: formatTableTime(s.computedAt, locale) })}</span> : null}
          <Button variant="secondary" size="xs" disabled={riskView.loading} aria-busy={riskView.loading || undefined} onClick={() => void riskView.refresh()}>
            {t("riskview.refresh")}
          </Button>
        </span>
      }
    >
      {riskView.error && !s ? <p className="ae-error">{`${t("riskview.failed")} ${riskView.error}`}</p> : null}
      {!s ? (
        <p className="ae-subtle">{riskView.loading ? t("riskview.loading") : t("workspace.loading")}</p>
      ) : books.length === 0 ? (
        <EmptyState title={t("riskview.empty")} />
      ) : (
        <>
          {books.length > 1 ? (
            <SegmentedControl
              label={t("riskview.bookLabel")}
              size="sm"
              value={bookId}
              options={books.map((b) => ({ value: b, label: t(`riskview.book.${b}`) }))}
              onChange={setBookSel}
            />
          ) : (
            <p className="ae-subtle">
              {t(`riskview.book.${bookId}`)} · {t("riskview.capital")}: {formatUsdt(book?.capital ?? 0, locale)}
            </p>
          )}
          <DataTable
            label={t("riskview.scenarios")}
            columns={scenarioColumns}
            rows={book?.scenarios ?? []}
            rowKey={(r) => String(r.btcMovePct)}
            isSelected={(r) => r.btcMovePct === move}
            onRowActivate={(r) => setMove(r.btcMovePct)}
            compact
          />
          {scenario && scenario.book.excluded > 0 ? (
            <p className="ae-banner" data-tone="warn" role="note">
              {t("riskview.excludedNote", { count: scenario.book.excluded })}
            </p>
          ) : null}
          {s.unpriced > 0 ? <p className="ae-subtle">{t("riskview.unpricedNote", { count: s.unpriced })}</p> : null}
          <div className="ae-setgrid">
            <Panel title={t("riskview.topFor", { move: moveLabel(move, locale) })}>
              {scenario && scenario.book.top.length > 0 ? (
                <DataTable label={t("riskview.topFor", { move: moveLabel(move, locale) })} columns={contribColumns} rows={scenario.book.top} rowKey={(r) => r.id} compact />
              ) : (
                <p className="ae-subtle">{t("riskview.noLosses")}</p>
              )}
            </Panel>
            <Panel title={t("riskview.concentration")}>
              <FactList
                rows={[
                  {
                    label: t("riskview.sameDirection"),
                    value: c?.sameDirectionPct == null ? NO_VALUE : `${pctText(c.sameDirectionPct, locale, 0)} ${c.dominant ? t(`riskview.dir.${c.dominant}`) : ""}`,
                  },
                  { label: t("riskview.topCoin"), value: c?.topCoin ? `${c.topCoin} · ${pctText(c.topCoinPct, locale, 0)}` : NO_VALUE },
                  {
                    label: t("riskview.avgBeta"),
                    value: c?.avgBeta == null ? NO_VALUE : formatNumber(c.avgBeta, locale, { minimumFractionDigits: 2, maximumFractionDigits: 2 }),
                  },
                ]}
              />
              {book && book.checks.length > 0 ? (
                <div className="ae-toolbar ae-riskview-checks" aria-label={t("riskview.checks")}>
                  {book.checks.map((ch) => (
                    <CheckChip key={ch.key} check={ch} />
                  ))}
                </div>
              ) : null}
            </Panel>
          </div>
          <Section level={3} title={t("riskview.exposures")} count={s.exposures.length}>
            <DataTable label={t("riskview.exposures")} columns={exposureColumns} rows={s.exposures} rowKey={(r) => r.id} compact />
          </Section>
          <p className="ae-subtle">{t("riskview.method", { days: s.betaDays })}</p>
        </>
      )}
      <ThresholdsForm />
    </Section>
  );
}

export function sourceLabel(t: (k: string) => string, source: ExposureRow["source"], label: string): string {
  if (source === "dca" || source === "grid") return `${t(`riskview.source.${source}`)} · ${label}`;
  return `${t(`riskview.source.${source}`)} · ${t(`bots.kind.${label}`)}`;
}

/** The three alert thresholds, stored with the risk settings. */
function ThresholdsForm() {
  const { t } = useTranslation();
  const { risk, riskView } = useDeskContext();
  const current = risk.state?.viewThresholds ?? null;
  const [draft, setDraft] = useState<Record<keyof ViewThresholds, string> | null>(null);
  const [saved, setSaved] = useState(false);

  useEffect(() => {
    if (current) {
      setDraft({
        sameDirectionPct: String(current.sameDirectionPct),
        singleCoinPct: String(current.singleCoinPct),
        stress20Pct: String(current.stress20Pct),
      });
    }
  }, [current?.sameDirectionPct, current?.singleCoinPct, current?.stress20Pct]);

  if (!current || !draft) return null;
  const parsed: ViewThresholds = {
    sameDirectionPct: Number(draft.sameDirectionPct),
    singleCoinPct: Number(draft.singleCoinPct),
    stress20Pct: Number(draft.stress20Pct),
  };
  const changed =
    parsed.sameDirectionPct !== current.sameDirectionPct ||
    parsed.singleCoinPct !== current.singleCoinPct ||
    parsed.stress20Pct !== current.stress20Pct;
  const field = (key: keyof ViewThresholds, min: number) => (
    <TextField
      label={t(`riskview.thresholds.${key}`)}
      type="number"
      inputMode="decimal"
      min={min}
      max={100}
      step={1}
      value={draft[key]}
      onChange={(e) => {
        setSaved(false);
        setDraft({ ...draft, [key]: e.target.value });
      }}
    />
  );
  return (
    <Panel title={t("riskview.thresholds.title")}>
      <div className="ae-riskview-thresholds">
        {field("sameDirectionPct", 50)}
        {field("singleCoinPct", 10)}
        {field("stress20Pct", 1)}
      </div>
      <div className="ae-toolbar">
        <Button
          size="sm"
          disabled={!changed || risk.busy}
          onClick={async () => {
            await risk.setViewThresholds(parsed);
            setSaved(true);
            void riskView.refresh();
          }}
        >
          {t("riskview.thresholds.save")}
        </Button>
        {saved && !risk.error ? <span className="ae-subtle">{t("riskview.thresholds.saved")}</span> : null}
        {risk.error ? <span className="ae-error">{risk.error}</span> : null}
      </div>
      <p className="ae-subtle">{t("riskview.thresholds.hint")}</p>
    </Panel>
  );
}
