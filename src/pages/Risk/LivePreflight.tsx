import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { useDeskContext } from "@/app/DeskProvider";
import { Link } from "@/app/router/router";
import { KEYS_PATH } from "@/app/router/routes";
import { Button } from "@/components/ui/Button/Button";
import { DataTable, type DataColumn } from "@/components/ui/DataTable/DataTable";
import { Panel } from "@/components/ui/Panel/Panel";
import { StatusChip, type StatusKind } from "@/components/ui/StatusChip/StatusChip";
import { localeForLanguage } from "@/i18n";
import { formatTableTime, NO_VALUE } from "@/lib/format";
import { errorMessage } from "@/lib/ipc/bridge";
import { exchangeName } from "@/lib/ipc/exchange/exchange";
import { livePreflight, type Preflight, type PreflightCheck, type PreflightCheckId, type PreflightVerdict } from "@/lib/ipc/app/preflight";
import { tradeVenues } from "@/lib/venues";

const CHIP: Record<PreflightVerdict, StatusKind> = { ok: "ok", warn: "needsSetup", fail: "error" };

/** Where a check that is not ok gets fixed inside the app. */
const FIX: Partial<Record<PreflightCheckId, string>> = {
  key: KEYS_PATH,
  account: KEYS_PATH,
  positions: "/positions?tab=exchange",
  membership: "/account",
  stream: "/account",
  liveBots: "/bots",
};

const REFUSED = "binanceRefused";

/**
 * Live readiness: one read-only pass over everything a real-money bot needs
 * (app/preflight.rs). Runs once on open and on demand.
 */
export function LivePreflight() {
  const { t, i18n } = useTranslation();
  const locale = localeForLanguage(i18n.resolvedLanguage ?? "en");
  const [result, setResult] = useState<Preflight | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const { vault, catalog } = useDeskContext();
  const venues = tradeVenues(vault.credentials);
  const [venuePick, setVenuePick] = useState<string | null>(null);
  const venue = venuePick ?? venues[0] ?? "binance";
  const exchange = exchangeName(result?.venue ?? venue, catalog.exchanges);

  const run = useCallback(async () => {
    setBusy(true);
    setError(null);
    try {
      setResult(await livePreflight(venue));
    } catch (e) {
      setError(errorMessage(e, "preflightFailed"));
    } finally {
      setBusy(false);
    }
  }, [venue]);

  useEffect(() => {
    void run();
  }, [run]);

  const reason = (c: PreflightCheck) => {
    if (!c.code) return null;
    if (c.code.startsWith(REFUSED)) return t("preflight.codes.binanceRefused", { code: c.code.slice(REFUSED.length), exchange });
    return t(`preflight.codes.${c.code}`, { defaultValue: c.code, exchange });
  };

  const columns: DataColumn<PreflightCheck>[] = [
    { id: "verdict", header: t("preflight.result"), width: "1%", cell: (c) => <StatusChip status={CHIP[c.verdict]} label={t(`preflight.verdict.${c.verdict}`)} /> },
    { id: "check", header: t("preflight.check"), cell: (c) => t(`preflight.checks.${c.id}`, { exchange }) },
    { id: "value", header: t("preflight.value"), numeric: true, priority: 2, cell: (c) => c.value ?? NO_VALUE },
    {
      id: "detail",
      header: t("preflight.detail"),
      cell: (c) => {
        const text = reason(c);
        const fix = c.verdict !== "ok" ? FIX[c.id] : undefined;
        return (
          <>
            {text ?? NO_VALUE}
            {fix ? (
              <>
                {" · "}
                <Link to={fix} className="ae-link">
                  {t("preflight.fix")}
                </Link>
              </>
            ) : null}
          </>
        );
      },
    },
  ];

  return (
    <Panel
      title={t("preflight.title")}
      aside={result ? <StatusChip status={CHIP[result.overall]} label={t(`preflight.overall.${result.overall}`)} /> : null}
    >
      <div className="ae-toolbar">
        {venues.length > 1 ? (
          <select className="ae-field__input" aria-label={t("preflight.exchange")} value={venue} onChange={(e) => setVenuePick(e.target.value)} disabled={busy}>
            {venues.map((v) => (
              <option key={v} value={v}>
                {exchangeName(v, catalog.exchanges)}
              </option>
            ))}
          </select>
        ) : null}
        <Button variant="secondary" size="sm" disabled={busy} onClick={() => void run()}>
          {busy ? t("preflight.running") : t("preflight.run")}
        </Button>
        {result ? <span className="ae-subtle">{t("preflight.checkedAt", { time: formatTableTime(result.checkedAtMs, locale) })}</span> : null}
      </div>
      {error ? (
        <p className="ae-error" role="alert">
          {t(`preflight.codes.${error}`, { defaultValue: error })}
        </p>
      ) : null}
      {result ? <DataTable label={t("preflight.title")} rows={result.checks} columns={columns} rowKey={(c) => c.id} compact /> : null}
      <p className="ae-subtle">{t("preflight.note")}</p>
    </Panel>
  );
}
