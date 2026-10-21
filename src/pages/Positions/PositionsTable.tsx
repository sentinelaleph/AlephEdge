import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import type { TFunction } from "i18next";
import { useNow } from "@/app/alerts";
import { useDeskContext } from "@/app/DeskProvider";
import { positionPnlColumns } from "@/components/PositionPnl/PositionPnl";
import { CLOSE_WORD } from "@/components/Account/AccountPanel/AccountPanel";
import { Button } from "@/components/ui/Button/Button";
import { Chip, LiveChip, ManualChip } from "@/components/ui/Chip/Chip";
import { ConfirmDialog } from "@/components/ui/ConfirmDialog/ConfirmDialog";
import { DataTable, type DataColumn } from "@/components/ui/DataTable/DataTable";
import { EmptyState } from "@/components/ui/EmptyState/EmptyState";
import { localeForLanguage } from "@/i18n";
import { formatAge, formatNumber, formatPrice, formatUsdt, NO_VALUE } from "@/lib/format";
import type { OpenPosition } from "@/lib/ipc/bot/bot";
import { marksByPosition } from "@/lib/positionMarks";
import "./Positions.css";

/**
 * Open positions as a dense table (bot detail, positions page). With
 * `onClose`, each row gets a Close action behind a confirmation: a paper
 * position asks for a click; a LIVE one says it sends a real market order and
 * waits for the typed CLOSE, like the exchange account's real closes.
 * Paper is the header's mode; only a real row carries a LIVE chip. With a
 * real row in the list, the table adds its quantity and the protective
 * orders the bot placed on the exchange (stop and take-profit order ids, or
 * none), read from the bot's own record of the position.
 */
export function PositionsTable({
  positions,
  showBot,
  emptyKey = "positions.noPaper",
  onClose,
}: {
  positions: OpenPosition[];
  showBot?: boolean;
  /** i18n key of the empty text (default: no open paper positions). */
  emptyKey?: string;
  /** Closes one position; resolves to the error text, or null once it is gone. */
  onClose?: (signalId: string, kind: OpenPosition["botKind"]) => Promise<string | null>;
}) {
  const { t, i18n } = useTranslation();
  const locale = localeForLanguage(i18n.resolvedLanguage ?? "en");
  const [target, setTarget] = useState<OpenPosition | null>(null);
  const [pending, setPending] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const rowKey = (p: OpenPosition) => `${p.botKind}:${p.signalId}`;
  const now = useNow(5_000);
  const { desk } = useDeskContext();
  const marks = useMemo(() => marksByPosition(desk.status), [desk.status]);

  const confirmClose = async () => {
    if (!target || !onClose) return;
    const p = target;
    setTarget(null);
    setError(null);
    setPending(rowKey(p));
    const err = await onClose(p.signalId, p.botKind);
    setPending(null);
    if (err) setError(`${p.symbol}: ${err}`);
  };

  const columns: DataColumn<OpenPosition>[] = [
    ...(showBot ? [{ id: "bot", header: t("table.bot"), priority: 2 as const, cell: (p: OpenPosition) => t(`bots.kind.${p.botKind}`) }] : []),
    {
      id: "symbol",
      header: t("table.symbol"),
      cell: (p) => (
        <span className="ae-postable__sym">
          {p.symbol}
          {p.live ? <LiveChip /> : null}
          {p.manual ? <ManualChip signalId={p.signalId} /> : null}
          {p.live && p.unprotected ? (
            <Chip tone="danger" title={t("bots.skipReasons.liveUnprotected")}>
              {t("pnl.exit.unprotected")}
            </Chip>
          ) : null}
        </span>
      ),
    },
    { id: "side", header: t("table.side"), cell: (p) => t(`signalDesk.direction.${p.direction === "short" ? "short" : "long"}`) },
    {
      id: "notional",
      header: t("table.notional"),
      numeric: true,
      priority: 2,
      cell: (p) => (p.notionalUsdt > 0 ? formatUsdt(p.notionalUsdt, locale, 0) : NO_VALUE),
    },
    { id: "entry", header: t("table.entry"), numeric: true, cell: (p) => formatPrice(p.entry, locale) },
    { id: "sl", header: t("table.sl"), numeric: true, cell: (p) => formatPrice(p.sl, locale) },
    { id: "tp", header: t("table.tp"), numeric: true, cell: (p) => formatPrice(p.tp, locale) },
    ...(positions.some((p) => p.live) ? protectionColumns(t, locale) : []),
    ...positionPnlColumns(t, locale, marks, now),
    { id: "leverage", header: t("table.leverage"), numeric: true, priority: 3, cell: (p) => `${p.leverage}x` },
    { id: "age", header: t("table.age"), numeric: true, priority: 2, cell: (p) => formatAge((now - p.openedAt) / 1000, locale) },
  ];

  return (
    <>
      {error ? (
        <p className="ae-error" role="alert">
          {error}
        </p>
      ) : null}
      <DataTable
        label={t("nav.positions")}
        columns={columns}
        rows={positions}
        rowKey={rowKey}
        rowTone={(p) => (p.live && p.unprotected ? "danger" : undefined)}
        empty={<EmptyState title={t(emptyKey)} />}
        actions={
          onClose
            ? (p) => (
                <Button
                  variant="secondary"
                  size="xs"
                  disabled={pending !== null}
                  aria-busy={pending === rowKey(p) || undefined}
                  onClick={() => setTarget(p)}
                >
                  {pending === rowKey(p) ? t("account.closing") : t("account.close")}
                </Button>
              )
            : undefined
        }
      />
      <ConfirmDialog
        open={target !== null}
        live={target?.live || undefined}
        title={target ? t("positions.closeBotTitle", { symbol: target.symbol, bot: t(`bots.kind.${target.botKind}`) }) : ""}
        body={target ? t(target.live ? "positions.closeLiveBody" : "positions.closePaperBody", { symbol: target.symbol }) : undefined}
        word={target?.live ? CLOSE_WORD : undefined}
        confirmLabel={t("account.close")}
        danger
        onCancel={() => setTarget(null)}
        onConfirm={() => void confirmClose()}
      />
    </>
  );
}

/**
 * A real position's quantity and its protective orders on the exchange: the
 * stop and take-profit order ids the bot placed (Binance algo ids), or "none"
 * in red. The bot's own record; the account is reconciled every 60 s and a
 * fired stop or take-profit closes the row.
 */
export function protectionColumns(t: TFunction, locale: string): DataColumn<OpenPosition>[] {
  const order = (id: number | null) => (id !== null && id !== undefined ? t("positions.protection.placed", { id: String(id) }) : t("positions.protection.none"));
  return [
    {
      id: "qty",
      header: t("positions.protection.qty"),
      numeric: true,
      priority: 2,
      cell: (p) => (p.live && p.qty > 0 ? formatNumber(p.qty, locale, { maximumFractionDigits: 8 }) : NO_VALUE),
    },
    {
      id: "stopOrder",
      header: t("positions.protection.stop"),
      headerTip: t("positions.protection.tip"),
      cell: (p) => (p.live ? order(p.stopAlgoId) : NO_VALUE),
      tone: (p) => (p.live && p.stopAlgoId === null ? "down" : undefined),
    },
    {
      id: "tpOrder",
      header: t("positions.protection.tp"),
      headerTip: t("positions.protection.tip"),
      priority: 2,
      cell: (p) => (p.live ? order(p.tpAlgoId) : NO_VALUE),
      tone: (p) => (p.live && p.tpAlgoId === null ? "down" : undefined),
    },
  ];
}
