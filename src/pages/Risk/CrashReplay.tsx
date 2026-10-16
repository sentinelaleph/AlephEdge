import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useDeskContext } from "@/app/DeskProvider";
import { Button } from "@/components/ui/Button/Button";
import { Chip } from "@/components/ui/Chip/Chip";
import { DataTable, type DataColumn } from "@/components/ui/DataTable/DataTable";
import { Section } from "@/components/ui/Section/Section";
import { SegmentedControl } from "@/components/ui/SegmentedControl/SegmentedControl";
import { localeForLanguage } from "@/i18n";
import { formatNumber, formatPnl, NO_VALUE, pnlToneAttr } from "@/lib/format";
import { errorMessage } from "@/lib/ipc/bridge";
import { riskviewReplay, type BookResult, type Contribution, type ReplayReport, type ReplayWindow } from "@/lib/ipc/riskview/riskview";
import { activeBooks, type BookId } from "@/lib/riskView";
import { riskviewErrorText } from "@/lib/riskViewText";
import { sourceLabel } from "./PortfolioRisk";

/**
 * Crash replay (center): today's open book walked through the real hourly
 * paths of past crash windows (riskview_replay). Run on demand: the first
 * run reads every coin's window from DataHub.
 */
export function CrashReplay() {
  const { t, i18n } = useTranslation();
  const locale = localeForLanguage(i18n.resolvedLanguage ?? "en");
  const { riskView } = useDeskContext();
  const [report, setReport] = useState<ReplayReport | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [selected, setSelected] = useState<ReplayWindow["id"] | null>(null);
  const [bookSel, setBookSel] = useState<BookId>("paper");
  const snap = riskView.snapshot;
  const books = snap ? activeBooks(snap) : [];
  const bookId: BookId = books.includes(bookSel) ? bookSel : (books[0] ?? "paper");

  const run = async () => {
    setBusy(true);
    setError(null);
    try {
      setReport(await riskviewReplay());
    } catch (e) {
      setError(errorMessage(e, t("riskview.failed")));
    } finally {
      setBusy(false);
    }
  };

  const pct = (v: number | null) => (v === null ? NO_VALUE : formatPnl(v, locale, { unit: "percent", digits: 1 }).text);
  const bookOf = (w: ReplayWindow): BookResult | null => (bookId === "real" ? w.real : w.paper);

  const columns: DataColumn<ReplayWindow>[] = [
    { id: "window", header: t("riskview.replay.col.window"), cell: (w) => t(`riskview.replay.window.${w.id}`) },
    { id: "btc", header: t("riskview.replay.col.btcMove"), numeric: true, priority: 2, cell: (w) => pct(w.btcMovePct), tone: (w) => pnlToneAttr(w.btcMovePct) },
    { id: "low", header: t("riskview.replay.col.btcLow"), numeric: true, priority: 3, cell: (w) => pct(w.btcLowPct), tone: (w) => pnlToneAttr(w.btcLowPct) },
    {
      id: "result",
      header: t("riskview.col.result"),
      numeric: true,
      keep: true,
      cell: (w) => {
        if (w.status !== "ok") {
          return (
            <span className="ae-subtle" title={w.error ? riskviewErrorText(t, w.error) : undefined}>
              {t("riskview.replay.unavailable")}
            </span>
          );
        }
        const b = bookOf(w);
        return b && b.included > 0 ? formatPnl(b.pnl, locale).text : NO_VALUE;
      },
      tone: (w) => {
        const b = bookOf(w);
        return w.status === "ok" && b && b.included > 0 ? pnlToneAttr(b.pnl) : undefined;
      },
    },
    { id: "pct", header: t("riskview.col.pctCapital"), numeric: true, cell: (w) => pct(bookOf(w)?.pctOfCapital ?? null), tone: (w) => pnlToneAttr(bookOf(w)?.pctOfCapital) },
    {
      id: "estimated",
      header: t("riskview.replay.col.estimated"),
      numeric: true,
      priority: 2,
      cell: (w) => {
        const b = bookOf(w);
        if (!b) return NO_VALUE;
        return b.excluded ? `${b.estimated} · ${t("riskview.col.excluded")}: ${b.excluded}` : b.estimated;
      },
    },
  ];

  const contribColumns: DataColumn<Contribution>[] = [
    {
      id: "symbol",
      header: t("riskview.col.symbol"),
      cell: (r) => (
        <span className="ae-namecell">
          {r.symbol}
          {r.basis === "beta" ? <Chip tone="warn">{t("riskview.replay.estimated")}</Chip> : null}
        </span>
      ),
    },
    { id: "bot", header: t("riskview.col.bot"), priority: 2, cell: (r) => sourceLabel(t, r.source, r.label) },
    { id: "pnl", header: t("riskview.col.result"), numeric: true, keep: true, cell: (r) => formatPnl(r.pnl, locale).text, tone: (r) => pnlToneAttr(r.pnl) },
    { id: "exit", header: t("riskview.col.exit"), priority: 2, cell: (r) => t(`riskview.exit.${r.exit ?? "open"}`, { defaultValue: r.exit ?? "" }) },
  ];

  const win = report?.windows.find((w) => w.id === selected) ?? null;
  const winBook = win ? bookOf(win) : null;

  return (
    <Section
      title={t("safety.section.replay")}
      aside={
        <span className="ae-toolbar">
          {report?.sample ? <Chip tone="accent">{t("riskview.sample")}</Chip> : null}
          <Button variant="secondary" size="xs" disabled={busy} aria-busy={busy || undefined} onClick={() => void run()}>
            {busy ? t("riskview.replay.running") : t("riskview.replay.run")}
          </Button>
        </span>
      }
    >
      {error ? <p className="ae-error">{error}</p> : null}
      {!report ? (
        <p className="ae-subtle">{t("riskview.replay.notRun")}</p>
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
          ) : null}
          <DataTable
            label={t("safety.section.replay")}
            columns={columns}
            rows={report.windows}
            rowKey={(w) => w.id}
            isSelected={(w) => w.id === selected}
            onRowActivate={(w) => setSelected(w.id === selected ? null : w.id)}
            compact
          />
          {win && win.status === "ok" ? (
            winBook && winBook.top.length > 0 ? (
              <DataTable
                label={t("riskview.replay.topFor", { window: t(`riskview.replay.window.${win.id}`) })}
                columns={contribColumns}
                rows={winBook.top}
                rowKey={(r) => r.id}
                compact
              />
            ) : (
              <p className="ae-subtle">{winBook ? t("riskview.noLosses") : t("riskview.replay.noBook")}</p>
            )
          ) : null}
          <p className="ae-subtle">
            {t("riskview.replay.method", { coverage: formatNumber(95, locale) })}
          </p>
        </>
      )}
    </Section>
  );
}
