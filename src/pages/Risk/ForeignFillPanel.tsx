import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/Button/Button";
import { DataTable, type DataColumn } from "@/components/ui/DataTable/DataTable";
import { Panel } from "@/components/ui/Panel/Panel";
import { StatusChip, type StatusKind } from "@/components/ui/StatusChip/StatusChip";
import { localeForLanguage } from "@/i18n";
import { formatNumber, formatTableTime, NO_VALUE } from "@/lib/format";
import { errorMessage } from "@/lib/ipc/bridge";
import type { ForeignFill, ForeignFillStatus } from "@/lib/ipc/risk/foreignFill";
import { refreshForeignFill, useForeignFill } from "@/lib/ipc/risk/useForeignFill";
import { SwitchRow } from "@/pages/Settings/TelegramPanel";

const REFUSED = "binanceRefused";

/** The watch's one-word state, for the panel chip. */
export function foreignFillState(s: ForeignFillStatus): { status: StatusKind; key: string } {
  if (s.alarms.length > 0) return { status: "error", key: "alarm" };
  if (!s.enabled) return { status: "needsSetup", key: "off" };
  return s.watching ? { status: "ok", key: "watching" } : { status: "needsSetup", key: "idle" };
}

/**
 * Fills on the Binance account that this app did not make (a leaked trade
 * key, or the user trading by hand). Live builds only: Risk, Live readiness.
 */
export function ForeignFillPanel() {
  const { t, i18n } = useTranslation();
  const locale = localeForLanguage(i18n.resolvedLanguage ?? "en");
  const { status, ack, setEnabled } = useForeignFill();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // Normally the desk's poll has filled the store; read once if not.
  useEffect(() => {
    if (!status) void refreshForeignFill();
  }, [status]);
  if (!status?.liveBuild) return null;

  const act = async (fn: () => Promise<void>) => {
    setBusy(true);
    setError(null);
    try {
      await fn();
    } catch (e) {
      setError(errorMessage(e, t("foreignFill.actionFailed")));
    } finally {
      setBusy(false);
    }
  };

  const state = foreignFillState(status);
  const reason = (code: string) =>
    code.startsWith(REFUSED)
      ? t("preflight.codes.binanceRefused", { code: code.slice(REFUSED.length), exchange: "Binance" })
      : t(`preflight.codes.${code}`, { defaultValue: code, exchange: "Binance" });

  const columns: DataColumn<ForeignFill>[] = [
    { id: "time", header: t("foreignFill.col.time"), cell: (f) => formatTableTime(f.timeMs, locale) },
    { id: "symbol", header: t("foreignFill.col.symbol"), cell: (f) => f.symbol },
    { id: "side", header: t("foreignFill.col.side"), cell: (f) => (f.side ? t(`foreignFill.side.${f.side === "BUY" ? "buy" : "sell"}`) : NO_VALUE) },
    { id: "qty", header: t("foreignFill.col.qty"), numeric: true, cell: (f) => (f.qty ? formatNumber(f.qty, locale, { maximumFractionDigits: 8 }) : NO_VALUE) },
    { id: "price", header: t("foreignFill.col.price"), numeric: true, priority: 2, cell: (f) => (f.price ? formatNumber(f.price, locale, { maximumFractionDigits: 8 }) : NO_VALUE) },
    { id: "trade", header: t("foreignFill.col.trade"), numeric: true, priority: 3, cell: (f) => f.tradeId },
  ];

  return (
    <Panel
      title={t("foreignFill.title")}
      tone={status.alarms.length > 0 ? "danger" : "default"}
      aside={<StatusChip status={state.status} label={t(`foreignFill.state.${state.key}`)} />}
    >
      <p className="ae-subtle">{t("foreignFill.desc")}</p>
      {status.alarms.length > 0 ? (
        <>
          <p className="ae-error" role="alert">
            {t("foreignFill.alarm")}
          </p>
          <DataTable label={t("foreignFill.title")} rows={[...status.alarms].reverse()} columns={columns} rowKey={(f) => `${f.symbol}:${f.tradeId}`} compact />
          <div className="ae-toolbar">
            <Button variant="secondary" size="sm" disabled={busy} onClick={() => void act(ack)}>
              {t("foreignFill.ack")}
            </Button>
            <span className="ae-subtle">{t("foreignFill.ackNote")}</span>
          </div>
        </>
      ) : null}
      <ul className="ae-notify">
        <SwitchRow
          id="foreign-fill-watch"
          name={t("foreignFill.switch")}
          desc={t("foreignFill.handTrades")}
          checked={status.enabled}
          disabled={busy}
          onChange={(on) => void act(() => setEnabled(on))}
        />
      </ul>
      {status.lastError ? (
        <p className="ae-error">{t("foreignFill.checkFailed", { reason: reason(status.lastError) })}</p>
      ) : status.lastCheckMs ? (
        <p className="ae-subtle">{t("foreignFill.lastCheck", { time: formatTableTime(status.lastCheckMs, locale) })}</p>
      ) : null}
      {error ? (
        <p className="ae-error" role="alert">
          {error}
        </p>
      ) : null}
    </Panel>
  );
}
