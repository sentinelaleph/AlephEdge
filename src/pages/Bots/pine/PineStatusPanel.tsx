import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/Button/Button";
import { LiveChip, PaperChip } from "@/components/ui/Chip/Chip";
import { FactList, Panel, type Fact } from "@/components/ui/Panel/Panel";
import { Icon } from "@/app/AppShell/icons";
import { localeForLanguage } from "@/i18n";
import { formatNumber, formatTableTime, formatUsdt, NO_VALUE } from "@/lib/format";
import { openHelpLink } from "@/lib/ipc/app/about";
import type { BotConfig } from "@/lib/ipc/bot/bot";
import type { PineStatus, PineSymbolStatus } from "@/lib/ipc/pine/pine";
import { shortHash } from "./pineBotModel";
import { engineMessage, errorKindText, pineCodeText, triggerLabel } from "./pineText";

interface Props {
  cfg: BotConfig | null;
  status: PineStatus | null;
  running: boolean;
  pilotLeft: number;
  marginUsed: number;
  /** The form differs from the saved bot: it runs the saved settings until Save. */
  unsaved?: boolean;
}

/** One symbol's last run: when, bars, run time, what fired, and the last error or note. */
function SymbolRun({ s }: { s: PineSymbolStatus }) {
  const { t, i18n } = useTranslation();
  const locale = localeForLanguage(i18n.resolvedLanguage ?? "en");
  const exists = (k: string) => i18n.exists(k);
  const k = (x: string) => `botsPine.${x}`;
  const n = (v: number | null) => (v === null ? NO_VALUE : formatNumber(v, locale));
  const e = s.lastError;
  const rows: Fact[] = [
    { label: t(k("lastRun")), value: s.lastRunAt ? formatTableTime(s.lastRunAt, locale) : t(k("notRunYet")) },
    { label: t(k("barsUsed")), value: n(s.barsUsed) },
    { label: t(k("runMs")), value: s.runMs === null ? NO_VALUE : t(k("ms"), { n: n(s.runMs) }) },
    {
      label: t(k("lastFired")),
      value: s.lastFired.length ? s.lastFired.map((f) => `${triggerLabel(t, f)} → ${t(k(`action.${f.action}`))}`).join(" · ") : t(k("firedNone")),
    },
  ];
  if (e) {
    rows.push({
      label: t(k("lastError")),
      tone: "danger",
      value: (
        <>
          {t(k("errorAt"), { kind: errorKindText(t, exists, e.kind), bar: formatTableTime(e.barTime, locale), line: e.line, col: e.col })}{" "}
          <code className="mono">{engineMessage(t, exists, e)}</code>
        </>
      ),
    });
  }
  if (s.lastNote) rows.push({ label: t(k("lastNote")), tone: "warn", value: pineCodeText(t, exists, s.lastNote) });
  return (
    <div className="ae-pine__symbol">
      <p className="ae-pine__symbolhead mono">{s.symbol}</p>
      <FactList rows={rows} />
    </div>
  );
}

/**
 * The Pine bot card: state, the halt and what ends it, entries today / max,
 * pilot entries left, the script hash; then each symbol's last run. Every
 * figure comes from `pine_status`; nothing is filled in.
 */
export function PineStatusPanel({ cfg, status, running, pilotLeft, marginUsed, unsaved }: Props) {
  const { t, i18n } = useTranslation();
  const locale = localeForLanguage(i18n.resolvedLanguage ?? "en");
  const exists = (k: string) => i18n.exists(k);
  const k = (x: string) => `botsPine.${x}`;
  const live = cfg?.mode === "live";
  const halted = status?.halted ?? null;

  const rows: Fact[] = [
    {
      label: t("botsTv.state"),
      value: !cfg ? t("botsTv.notSaved") : running ? t("settings.tv.botRunning") : t("settings.tv.botStopped"),
    },
    ...(live ? [{ label: t(k("armed")), value: cfg?.live ? t(k("armedYes")) : t(k("armedNo")), tone: cfg?.live ? undefined : ("warn" as const) }] : []),
    { label: t(k("hash")), value: <span className="mono">{shortHash(cfg?.pine?.scriptHash) || NO_VALUE}</span> },
    { label: t(k("timeframe")), value: cfg?.pine?.timeframe ?? NO_VALUE },
    {
      label: t(k("entriesToday")),
      value: status && cfg ? `${formatNumber(status.opensToday, locale)} / ${formatNumber(status.maxOpensPerDay, locale)}` : NO_VALUE,
    },
    { label: t("botsTv.marginUsed"), value: cfg?.pine ? `${formatUsdt(marginUsed, locale)} / ${formatUsdt(cfg.pine.budgetUsdt, locale)}` : NO_VALUE },
    ...(live ? [{ label: t("botsTv.pilot"), value: formatNumber(pilotLeft, locale) }] : []),
    ...(status && status.pending > 0 ? [{ label: t(k("pending")), value: formatNumber(status.pending, locale), tone: "warn" as const }] : []),
    { label: t(k("failedInARow")), value: status ? formatNumber(status.failedInARow, locale) : NO_VALUE, tone: status && status.failedInARow > 0 ? ("warn" as const) : undefined },
    { label: t(k("nextBar")), value: running && status?.nextBarClose ? formatTableTime(status.nextBarClose, locale) : NO_VALUE },
  ];

  return (
    <>
      <Panel title={t("botsTv.now")} aside={live ? <LiveChip /> : cfg ? <PaperChip /> : null}>
        {halted ? (
          <p className="ae-banner" data-tone="danger" role="alert">
            {pineCodeText(t, exists, halted)} {t(k("haltedFix"))}
          </p>
        ) : null}
        {unsaved ? (
          <p className="ae-banner" data-tone="warn" role="status" data-testid="pine-unsaved">
            {t(k("unsaved"))}
          </p>
        ) : null}
        <FactList rows={rows} />
        <p className="ae-newbot__links">
          <Button size="sm" variant="secondary" onClick={() => void openHelpLink("chart").catch(() => undefined)}>
            {t(k("openChart"))}
            <Icon name="external" size={12} />
          </Button>
        </p>
        <p className="ae-subtle">{t(k("openChartNote"), { symbol: cfg?.pine?.symbols[0] ?? "BTCUSDT", timeframe: cfg?.pine?.timeframe ?? "1h" })}</p>
      </Panel>
      {status && status.symbols.length > 0 ? (
        <Panel title={t(k("runs"))}>
          {status.symbols.map((s) => (
            <SymbolRun key={s.symbol} s={s} />
          ))}
        </Panel>
      ) : null}
      <Panel title={t(k("rulesTitle"))}>
        <ul className="ae-setlist">
          {["rule1", "rule2", "rule3", "rule4"].map((r) => (
            <li key={r}>{t(k(r))}</li>
          ))}
        </ul>
      </Panel>
    </>
  );
}
