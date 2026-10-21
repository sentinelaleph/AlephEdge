import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { useDeskContext } from "@/app/DeskProvider";
import { ConfirmDialog } from "@/components/ui/ConfirmDialog/ConfirmDialog";
import { FactList, type Fact } from "@/components/ui/Panel/Panel";
import { SegmentedControl } from "@/components/ui/SegmentedControl/SegmentedControl";
import { localeForLanguage } from "@/i18n";
import { formatNumber, formatUsdt } from "@/lib/format";
import { LIVE_CONFIRMATION, type LiveTerms, type TradeMode } from "@/lib/ipc/bot/bot";
import { exchangeName } from "@/lib/ipc/exchange/exchange";
import type { Blocker } from "@/lib/liveMode";
import { liveTargetKey } from "@/lib/venues";
import "./LiveMode.css";

/**
 * Paper or Live on a creation screen. Paper is the default and Live is never
 * pre-selected by this control; Live is offered only when nothing blocks it,
 * and the blockers are listed under the control with what fixes them.
 */
export function ModePicker({
  value,
  onChange,
  blockers,
  disabled,
  liveNote,
}: {
  value: TradeMode;
  onChange: (m: TradeMode) => void;
  blockers: Blocker[];
  disabled?: boolean;
  /** What Live means here (one line under the control while Live is chosen). */
  liveNote?: ReactNode;
}) {
  const { t } = useTranslation();
  const texts = blockers.map((b) => t(b.key, b.params));
  return (
    <div className="ae-livemode">
      <SegmentedControl
        label={t("liveMode.label")}
        value={value}
        size="sm"
        disabled={disabled}
        onChange={onChange}
        options={[
          { value: "paper", label: t("liveMode.paper") },
          { value: "live", label: t("liveMode.live"), disabled: texts.length > 0, reason: texts.join(" · ") },
        ]}
      />
      {value === "paper" && texts.length > 0 ? (
        <ul className="ae-livemode__blockers" aria-label={t("liveMode.blockersTitle")}>
          {texts.map((x) => (
            <li key={x}>{x}</li>
          ))}
        </ul>
      ) : null}
      {value === "live" && liveNote ? <p className="ae-livemode__note">{liveNote}</p> : null}
    </div>
  );
}

/** The money one confirmation puts at risk, as label / value rows. */
export function LiveTermsFacts({ terms, extra = [] }: { terms: LiveTerms; extra?: Fact[] }) {
  const { t, i18n } = useTranslation();
  const locale = localeForLanguage(i18n.resolvedLanguage ?? "en");
  const { desk, catalog } = useDeskContext();
  const s = desk.status;
  const venue = exchangeName(terms.venue, catalog.exchanges);
  const rows: Fact[] = [
    { label: t("liveMode.terms.orders"), value: t(liveTargetKey(terms.venue, s.binanceIsProduction, s.venueSandbox), { exchange: venue }) },
    { label: t("liveMode.terms.margin"), value: formatUsdt(terms.marginAtRiskUsdt, locale), tone: "danger", info: t("liveMode.terms.marginInfo") },
    {
      label: t("liveMode.terms.notional"),
      value:
        terms.maxPositionMarginUsdt !== undefined
          ? t("liveMode.terms.notionalValue", {
              notional: formatUsdt(terms.maxNotionalUsdt, locale),
              margin: formatUsdt(terms.maxPositionMarginUsdt, locale),
            })
          : formatUsdt(terms.maxNotionalUsdt, locale),
      info: t("liveMode.terms.notionalInfo"),
    },
    { label: t("liveMode.terms.leverage"), value: `${formatNumber(terms.leverage, locale)}x` },
    ...(terms.positions > 1 ? [{ label: t("liveMode.terms.positions"), value: formatNumber(terms.positions, locale) }] : []),
    ...extra,
    {
      label: t("liveMode.terms.pilot"),
      value: t("liveMode.terms.pilotValue", { count: terms.pilotCount, cap: formatUsdt(terms.pilotNotionalUsdt, locale, 0) }),
    },
  ];
  return <FactList rows={rows} />;
}

/**
 * The typed LIVE with the money at risk shown. `onConfirm` gets the typed
 * word; the caller sends it with `terms.marginAtRiskUsdt`, which Rust
 * re-checks against the settings in force.
 */
export function LiveConfirmDialog({
  open,
  title,
  terms,
  extra,
  note,
  confirmLabel,
  busy,
  onConfirm,
  onCancel,
}: {
  open: boolean;
  title: string;
  terms: LiveTerms | null;
  extra?: Fact[];
  /** One line under the figures (what happens after a restart, what the arm covers). */
  note?: ReactNode;
  confirmLabel: string;
  busy?: boolean;
  onConfirm: (typed: string) => void;
  onCancel: () => void;
}) {
  return (
    <ConfirmDialog
      open={open && terms !== null}
      live
      danger
      title={title}
      body={
        terms ? (
          <>
            <LiveTermsFacts terms={terms} extra={extra} />
            {note ? <p className="ae-livemode__note">{note}</p> : null}
          </>
        ) : null
      }
      word={LIVE_CONFIRMATION}
      confirmLabel={confirmLabel}
      busy={busy}
      onConfirm={onConfirm}
      onCancel={onCancel}
    />
  );
}
