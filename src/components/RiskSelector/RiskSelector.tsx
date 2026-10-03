import { useState } from "react";
import { AnimatePresence, motion } from "framer-motion";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/Button/Button";
import { Panel } from "@/components/ui/Panel/Panel";
import { SegmentedControl } from "@/components/ui/SegmentedControl/SegmentedControl";
import { TextField } from "@/components/ui/TextField/TextField";
import { localeForLanguage } from "@/i18n";
import { formatDecimalInput, parseDecimal } from "@/lib/decimal";
import { formatNumber } from "@/lib/format";
import { RISK_LEVELS, RISK_LIMITS_TABLE, type RiskLevel } from "@/lib/ipc/risk/risk";
import { collapseTransition } from "@/lib/motion";
import type { RiskController } from "./useRisk";
import "./RiskSelector.css";

interface RiskSelectorProps {
  risk: RiskController;
}

/**
 * The global risk level selector (PRD §5.3): each level carries its own
 * colour as a small dot next to full-contrast text, and the selected level's
 * hard limits are spelled out underneath — numbers, not vibes. Below it: the
 * simulated balance the % caps apply to, and the daily-stop close preference.
 * Picking Greedy demands an explicit confirmation with the numeric consequences.
 */
export function RiskSelector({ risk }: RiskSelectorProps) {
  const { t, i18n } = useTranslation();
  const locale = localeForLanguage(i18n.language);
  const greedy = RISK_LIMITS_TABLE.greedy;
  const [pendingGreedy, setPendingGreedy] = useState(false);
  const [balanceDraft, setBalanceDraft] = useState<string | null>(null);
  const [lossDraft, setLossDraft] = useState<string | null>(null);
  const [lossInvalid, setLossInvalid] = useState(false);
  const s = risk.state;
  const limits = s?.limits;

  async function pick(level: RiskLevel) {
    if (level === "greedy" && limits?.level !== "greedy") {
      setPendingGreedy(true);
      return;
    }
    setPendingGreedy(false);
    await risk.selectLevel(level);
  }

  function commitBalance() {
    if (balanceDraft === null) return;
    const value = parseDecimal(balanceDraft);
    setBalanceDraft(null);
    if (Number.isFinite(value) && value > 0 && value !== s?.balance) {
      void risk.setBalance(value);
    }
  }

  // Removing the override is its own button (clearDailyLoss), never an empty
  // field: clearing the text used to drop the user's limit silently. An empty
  // field now just reverts to the stored value. Non-positive or unreadable
  // input stays in the field, marked invalid, and is not stored: 0% would mean
  // "stop after losing nothing", which halts the desk instead of protecting it.
  function commitDailyLoss() {
    if (lossDraft === null) return;
    const raw = lossDraft.trim();
    if (raw === "") {
      setLossDraft(null);
      setLossInvalid(false);
      return;
    }
    const value = parseDecimal(raw);
    if (!(Number.isFinite(value) && value > 0)) {
      setLossInvalid(true);
      return;
    }
    setLossDraft(null);
    setLossInvalid(false);
    if (value !== s?.dailyLossOverridePct) void risk.setDailyLoss(value);
  }

  function clearDailyLoss() {
    setLossDraft(null);
    setLossInvalid(false);
    void risk.setDailyLoss(null);
  }

  return (
    <Panel title={t("risk.title")} className="ae-risk">
      <SegmentedControl
        label={t("risk.title")}
        value={limits?.level ?? null}
        fill
        wrap
        disabled={risk.busy || !s}
        options={RISK_LEVELS.map((level) => ({
          value: level,
          label: t(`risk.levels.${level}`),
          indicator: `var(--risk-${level})`,
        }))}
        onChange={(level) => void pick(level)}
      />

      {risk.error ? (
        <div className="ae-risk__error" role="alert">
          <p>{risk.error}</p>
          {!s ? (
            <Button variant="secondary" size="sm" onClick={risk.retry}>
              {t("common.retry")}
            </Button>
          ) : null}
        </div>
      ) : !s ? (
        <p className="ae-risk__limits">{t("workspace.loading")}</p>
      ) : null}

      {s && limits ? (
        <p className="ae-risk__limits">
          {t("risk.limitsLine", {
            leverage: limits.maxLeverage,
            positions: limits.maxConcurrentPositions,
            capital: Math.round(s.maxCapitalQuote),
            dailyLoss: s.effectiveDailyLossPct,
          })}
        </p>
      ) : null}

      <div className="ae-risk__controls">
        <TextField
          label={t("risk.balance")}
          type="text"
          inputMode="decimal"
          value={balanceDraft ?? formatDecimalInput(s?.balance)}
          onChange={(e) => setBalanceDraft(e.target.value)}
          onBlur={commitBalance}
          disabled={risk.busy || !s}
          hint={t("risk.balanceHint")}
        />
        <div className="ae-risk__loss">
          {/* The rule (stricter honoured, looser capped) lives in the info
              tip; the field itself carries only the figure in force. */}
          <TextField
            label={t("risk.dailyLoss")}
            info={t("risk.dailyLossHint", { cap: limits?.dailyLossLimitPct ?? 0 })}
            type="text"
            inputMode="decimal"
            placeholder={formatDecimalInput(limits?.dailyLossLimitPct)}
            value={lossDraft ?? formatDecimalInput(s?.dailyLossOverridePct)}
            onChange={(e) => {
              setLossDraft(e.target.value);
              setLossInvalid(false);
            }}
            onBlur={commitDailyLoss}
            disabled={risk.busy || !s}
            aria-invalid={lossInvalid || undefined}
            hint={
              lossInvalid
                ? t("risk.dailyLossInvalid")
                : s?.dailyLossOverrideCapped
                  ? t("risk.dailyLossCapped", {
                      typed: s.dailyLossOverridePct,
                      cap: limits?.dailyLossLimitPct,
                    })
                  : t("risk.dailyLossCapHint", { cap: limits?.dailyLossLimitPct ?? 0 })
            }
          />
          {s?.dailyLossOverridePct != null ? (
            <Button variant="secondary" size="sm" disabled={risk.busy} onClick={clearDailyLoss}>
              {t("risk.dailyLossClear", { cap: limits?.dailyLossLimitPct ?? 0 })}
            </Button>
          ) : null}
        </div>
        <label className="ae-risk__toggle">
          <input
            type="checkbox"
            checked={s?.closeOnStop ?? true}
            onChange={(e) => void risk.setCloseOnStop(e.target.checked)}
            disabled={risk.busy || !s}
          />
          <span>{t("risk.closeOnStop")}</span>
        </label>
      </div>

      <AnimatePresence initial={false}>
        {pendingGreedy ? (
          <motion.div
            className="ae-risk__confirm"
            initial={{ opacity: 0, height: 0 }}
            animate={{ opacity: 1, height: "auto" }}
            exit={{ opacity: 0, height: 0 }}
            transition={collapseTransition()}
          >
            <p className="ae-risk__confirm-text">
              {t("risk.greedyWarning", {
                maxLeverage: formatNumber(greedy.maxLeverage, locale),
                maxPositions: formatNumber(greedy.maxConcurrentPositions, locale),
                dailyLoss: formatNumber(greedy.dailyLossLimitPct, locale),
              })}
            </p>
            <div className="ae-risk__confirm-actions">
              <Button
                size="sm"
                disabled={risk.busy}
                onClick={() => {
                  setPendingGreedy(false);
                  void risk.selectLevel("greedy");
                }}
              >
                {t("risk.greedyConfirm")}
              </Button>
              <Button variant="secondary" size="sm" onClick={() => setPendingGreedy(false)}>
                {t("risk.greedyCancel")}
              </Button>
            </div>
          </motion.div>
        ) : null}
      </AnimatePresence>
    </Panel>
  );
}
