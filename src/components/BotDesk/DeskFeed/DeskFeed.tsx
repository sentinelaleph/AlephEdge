import { useTranslation } from "react-i18next";
import { Chip, LiveChip, ManualChip } from "@/components/ui/Chip/Chip";
import { DataTable, type DataColumn } from "@/components/ui/DataTable/DataTable";
import { Section } from "@/components/ui/Section/Section";
import { localeForLanguage } from "@/i18n";
import { formatDuration, formatNumber, formatPrice, formatTableTime, formatUsdt, NO_VALUE } from "@/lib/format";
import { isLivePosition, type OpenPosition, type SkipNote } from "@/lib/ipc/bot/bot";
import { skipParams } from "@/lib/skipNotes";
import { targetLabel } from "@/lib/takeProfit";
import "./DeskFeed.css";

interface DeskFeedProps {
  positions: OpenPosition[];
  skips: SkipNote[];
}

/** How many refusals the desk shows; the bot's Log tab has the rest. */
const SKIPS_SHOWN = 8;

/**
 * Live activity: open positions and, just as visible, every trade the bot
 * REFUSED with its reason (PRD §5.4: skipped trades are never silent).
 * LIVE marks a real-money row; paper is the header's mode.
 */
export function DeskFeed({ positions, skips }: DeskFeedProps) {
  const { t, i18n } = useTranslation();
  const locale = localeForLanguage(i18n.resolvedLanguage ?? "en");
  const custom = t("bots.tp.custom");

  const positionColumns: DataColumn<OpenPosition>[] = [
    {
      id: "symbol",
      header: t("table.symbol"),
      cell: (p) => (
        <span className="ae-deskfeed__symbol">
          <span className="ae-deskfeed__dir" data-direction={p.direction} aria-label={t(`bots.dir.${p.direction === "short" ? "short" : "long"}`)}>
            {p.direction === "short" ? "▼" : "▲"}
          </span>
          {p.symbol}
          {isLivePosition(p) ? <LiveChip /> : null}
          {p.manual ? <ManualChip /> : null}
        </span>
      ),
    },
    { id: "bot", header: t("table.bot"), priority: 2, cell: (p) => t(`bots.kind.${p.botKind}`) },
    { id: "entry", header: t("table.entry"), numeric: true, cell: (p) => formatPrice(p.entry, locale) },
    { id: "tp", header: t("table.tp"), numeric: true, cell: (p) => formatPrice(p.tp, locale) },
    { id: "sl", header: t("table.sl"), numeric: true, cell: (p) => formatPrice(p.sl, locale) },
    { id: "lev", header: t("table.leverage"), numeric: true, priority: 3, cell: (p) => `${p.leverage}x` },
    {
      id: "target",
      header: t("bots.tp.title"),
      priority: 2,
      cell: (p) =>
        p.tpFallbackFrom ? (
          <span title={t("bots.position.fallbackTooltip", { from: targetLabel(p.tpFallbackFrom, custom) })}>
            {t("bots.position.fallback", { from: targetLabel(p.tpFallbackFrom, custom), to: targetLabel(p.tpTarget, custom) })}
          </span>
        ) : (
          targetLabel(p.tpTarget, custom)
        ),
    },
    {
      id: "notional",
      header: t("table.notional"),
      numeric: true,
      priority: 3,
      // 0 on rows opened before sizing existed: not sized this way, not a zero size.
      cell: (p) =>
        p.notionalUsdt > 0 ? (
          <span
            title={
              p.sizingMode === "risk" && p.riskPct != null
                ? t("bots.position.sizingLineRisk", {
                    pct: p.riskPct,
                    amount: Math.round(p.notionalUsdt),
                    leverage: p.effectiveLeverage.toFixed(2),
                  })
                : t("bots.position.sizingLineFixed", {
                    amount: Math.round(p.notionalUsdt),
                    leverage: p.effectiveLeverage.toFixed(2),
                  })
            }
          >
            {formatUsdt(p.notionalUsdt, locale, 0)}
          </span>
        ) : (
          NO_VALUE
        ),
    },
    {
      id: "flags",
      header: t("table.state"),
      priority: 2,
      cell: (p) => {
        const flags = [
          p.riskCapped ? (
            <Chip key="cap" tone="warn" title={t("bots.position.riskCappedTooltip")}>
              {t("bots.position.riskCappedBadge")}
            </Chip>
          ) : null,
          p.breakevenArmed ? (
            <Chip key="be" tone="success" title={t("bots.position.breakevenTooltip")}>
              {t("bots.position.breakevenBadge")}
            </Chip>
          ) : null,
          p.stopAtBreakeven ? (
            <Chip key="xbe" tone="success" title={t("bots.position.stopAtBreakevenTooltip")}>
              {t("bots.position.stopAtBreakevenBadge")}
            </Chip>
          ) : null,
          // horizonMs is an absolute UNIX-ms deadline (expires_at + 72h).
          p.horizonMs > 0 ? (
            <Chip
              key="hz"
              title={p.tpTarget && p.tpTarget !== "tp1" ? t("bots.position.horizonExit") : t("bots.position.horizonTooltip")}
            >
              {t("bots.position.horizonLeft", { time: formatDuration(p.horizonMs - Date.now(), locale) })}
            </Chip>
          ) : null,
          p.partialPrice != null ? (
            <Chip key="part">
              {t("bots.position.partial", { pct: Math.round(p.partialFraction * 100), price: formatPrice(p.partialPrice, locale) })}
            </Chip>
          ) : null,
        ].filter(Boolean);
        return flags.length ? <span className="ae-deskfeed__flags">{flags}</span> : NO_VALUE;
      },
    },
  ];

  const skipColumns: DataColumn<SkipNote>[] = [
    { id: "time", header: t("table.time"), width: "1%", cell: (s) => formatTableTime(s.atMs, locale) },
    { id: "symbol", header: t("table.symbol"), width: "1%", cell: (s) => s.symbol },
    {
      id: "reason",
      header: t("table.reason"),
      wrap: true,
      keep: true,
      cell: (s) => t(`bots.skipReasons.${s.reason}`, skipParams(s)),
    },
  ];

  return (
    <>
      <Section title={t("bots.openPositions")} count={formatNumber(positions.length, locale)}>
        <DataTable
          label={t("bots.openPositions")}
          columns={positionColumns}
          rows={positions}
          rowKey={(p) => `${p.botKind}:${p.signalId}`}
          compact
          empty={<span className="ae-subtle">{t("bots.noPositions")}</span>}
        />
      </Section>
      <Section title={t("bots.skips")} count={skips.length > SKIPS_SHOWN ? `${SKIPS_SHOWN} / ${skips.length}` : skips.length}>
        <DataTable
          label={t("bots.skips")}
          columns={skipColumns}
          rows={skips.slice(0, SKIPS_SHOWN)}
          rowKey={(s) => `${s.atMs}:${s.symbol}:${s.reason}`}
          compact
          empty={<span className="ae-subtle">{t("bots.noSkips")}</span>}
        />
      </Section>
    </>
  );
}
