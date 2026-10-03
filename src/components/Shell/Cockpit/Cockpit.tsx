import { useState } from "react";
import { useTranslation } from "react-i18next";
import { ConnectionPill, type ConnectionState } from "@/components/ui/ConnectionPill/ConnectionPill";
import { FactList, type Fact } from "@/components/ui/Panel/Panel";
import { localeForLanguage } from "@/i18n";
import { localizeError } from "@/lib/errorText";
import { formatAge, formatLatency } from "@/lib/format";
import { errorMessage } from "@/lib/ipc/bridge";
import { cockpitExchange, cockpitSentinel, type PingResult } from "@/lib/ipc/cockpit/cockpit";
import { exchangeName } from "@/lib/ipc/exchange/exchange";
import type { HealthLevel } from "@/lib/ipc/health/health";
import { useExchangeCatalog } from "@/lib/ipc/exchange/useExchangeCatalog";
import "./Cockpit.css";

/** What the desk already knows without probing (header only; the gate bar has none). */
export interface CockpitReadings {
  sentinel?: {
    state: ConnectionState;
    /** Short state word ("Connected", "Reconnecting"). */
    label: string;
    latencyMs?: number | null;
    lastSignalSecs?: number | null;
  };
  exchange?: { level: HealthLevel; latencyMs: number | null } | null;
}

interface CockpitProps {
  /** Connected exchange to probe; null shows "Exchange · no key". */
  exchangeId: string | null;
  readings?: CockpitReadings;
}

type Probe = { state: "idle" | "checking" | "done"; result?: PingResult; at?: number };

const LEVEL_STATE: Record<HealthLevel, ConnectionState> = { ok: "ok", warn: "warn", down: "down", unknown: "idle" };

/**
 * The two connection pills in the header (and the gate bar): the connected
 * exchange and Sentinel. Each shows dot + name + one metric; the full detail
 * (stream state, latency, last signal, the last on-demand check's result) is
 * in the tooltip. A click re-runs the check, which exercises the exact
 * stream-ticket step the live feed needs.
 */
export function Cockpit({ exchangeId, readings }: CockpitProps) {
  const { t, i18n } = useTranslation();
  const locale = localeForLanguage(i18n.resolvedLanguage ?? "en");
  const { exchanges } = useExchangeCatalog(t("common.exchangeListFailed"));
  const [ex, setEx] = useState<Probe>({ state: "idle" });
  const [se, setSe] = useState<Probe>({ state: "idle" });

  const run = async (set: (p: Probe) => void, probe: () => Promise<PingResult>) => {
    set({ state: "checking" });
    try {
      set({ state: "done", result: await probe(), at: Date.now() });
    } catch (e) {
      set({ state: "done", result: { ok: false, detail: errorMessage(e, t("cockpit.error")) }, at: Date.now() });
    }
  };

  const checkRow = (p: Probe): Fact[] =>
    p.result
      ? [
          {
            label: t("cockpit.lastCheck"),
            value: `${localizeError(p.result.detail)}${p.result.latencyMs != null ? ` · ${formatLatency(p.result.latencyMs, locale)}` : ""}`,
          },
        ]
      : [];

  // ---- Sentinel ----
  const sr = readings?.sentinel;
  const sBase: ConnectionState = sr?.state ?? (se.result ? (se.result.ok ? "ok" : "down") : "idle");
  const sFailed = se.state === "done" && se.result && !se.result.ok;
  const sState: ConnectionState =
    se.state === "checking" ? "checking" : sFailed ? (sBase === "ok" ? "warn" : "down") : sBase;
  const sLatency = (se.result?.ok ? se.result.latencyMs : undefined) ?? sr?.latencyMs ?? null;
  const sMetric =
    se.state === "checking"
      ? t("cockpit.checking")
      : sFailed
        ? t("cockpit.failed")
        : sLatency != null
          ? formatLatency(sLatency, locale)
          : sr?.label;
  const sRows: Fact[] = [
    ...(sr ? [{ label: t("cockpit.stream"), value: sr.label }] : []),
    ...(sr?.latencyMs != null ? [{ label: t("statusbar.latency"), value: formatLatency(sr.latencyMs, locale) }] : []),
    ...(sr?.lastSignalSecs != null ? [{ label: t("statusbar.lastEventLabel"), value: formatAge(sr.lastSignalSecs, locale) }] : []),
    ...checkRow(se),
  ];

  // ---- Exchange ----
  const exName = exchangeId ? exchangeName(exchangeId, exchanges) : t("cockpit.exchange");
  const xr = readings?.exchange ?? null;
  const xFailed = ex.state === "done" && ex.result && !ex.result.ok;
  const xBase: ConnectionState = xr ? LEVEL_STATE[xr.level] : ex.result?.ok ? "ok" : "idle";
  const xState: ConnectionState = !exchangeId
    ? "idle"
    : ex.state === "checking"
      ? "checking"
      : xFailed
        ? "down"
        : xBase;
  const xLatency = (ex.result?.ok ? ex.result.latencyMs : undefined) ?? xr?.latencyMs ?? null;
  const xMetric = !exchangeId
    ? t("cockpit.noKey")
    : ex.state === "checking"
      ? t("cockpit.checking")
      : xFailed
        ? t("cockpit.failed")
        : xLatency != null
          ? formatLatency(xLatency, locale)
          : undefined;
  const xRows: Fact[] = !exchangeId
    ? [{ label: t("cockpit.exchangeKey"), value: t("cockpit.none") }]
    : [
        ...(xr?.latencyMs != null ? [{ label: t("statusbar.latency"), value: formatLatency(xr.latencyMs, locale) }] : []),
        ...checkRow(ex),
      ];

  const tip = (title: string, rows: Fact[], clickable: boolean) => (
    <>
      <span className="ae-tip__title">{title}</span>
      {rows.length > 0 ? <FactList rows={rows} /> : null}
      {clickable ? <span className="ae-cockpit__hint">{t("cockpit.clickToCheck")}</span> : null}
    </>
  );

  return (
    <div className="ae-cockpit" role="group" aria-label={t("cockpit.title")}>
      <ConnectionPill
        name={exName}
        state={xState}
        metric={xMetric}
        unavailable={!exchangeId}
        detail={tip(exName, xRows, !!exchangeId)}
        onClick={() => exchangeId && run(setEx, () => cockpitExchange(exchangeId))}
      />
      <ConnectionPill
        // "Sentinel" is a brand name (ribqa.com's signal engine), not a
        // string to translate.
        name="Sentinel"
        state={sState}
        metric={sMetric}
        detail={tip("Sentinel", sRows, true)}
        onClick={() => run(setSe, cockpitSentinel)}
      />
    </div>
  );
}
