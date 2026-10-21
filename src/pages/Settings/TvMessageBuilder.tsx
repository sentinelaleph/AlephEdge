import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/Button/Button";
import { Panel } from "@/components/ui/Panel/Panel";
import { TextField } from "@/components/ui/TextField/TextField";
import { Segmented } from "@/pages/Bots/strategy/formControls";
import {
  ALERT_MESSAGE,
  buildMessage,
  CLOSE,
  DEFAULT_FORM,
  MARKET_POSITION,
  ORDER_ACTION,
  TIMENOW,
  type BuilderForm,
  type IdKind,
  type LevelKind,
  type ManualAction,
} from "@/lib/tvMessage";

/** TradingView placeholders the notes name (never i18n params of their own). */
const PLACEHOLDERS = { close: CLOSE, timenow: TIMENOW, action: ORDER_ACTION, position: MARKET_POSITION, alertMessage: ALERT_MESSAGE };

/**
 * The alert message builder: a form that writes the JSON for TradingView's
 * Message box (src/lib/tvMessage.ts). Nothing typed here is saved: the
 * passphrase goes into the copied text only.
 */
export function TvMessageBuilder({ onCopy, copyLabel }: { onCopy: (text: string) => void; copyLabel: string }) {
  const { t } = useTranslation();
  const [f, setF] = useState<BuilderForm>(DEFAULT_FORM);
  const set = (patch: Partial<BuilderForm>) => setF((cur) => ({ ...cur, ...patch }));
  const built = buildMessage(f);
  const strategy = f.mode === "strategy";
  const entry = strategy || f.action !== "close";
  const k = (s: string) => `settings.tv.builder.${s}`;

  return (
    <Panel title={t(k("title"))}>
      <div className="ae-tvb">
        <div className="ae-tvb__row">
          <Segmented
            label={t(k("mode"))}
            value={f.mode}
            onChange={(mode) => set({ mode })}
            options={[
              { value: "manual", label: t(k("manual")) },
              { value: "strategy", label: t(k("strategy")) },
            ]}
          />
          {strategy ? null : (
            <Segmented<ManualAction>
              label={t(k("action"))}
              value={f.action}
                onChange={(action) => set({ action })}
              options={[
                { value: "buy", label: t("settings.tv.action.buy") },
                { value: "sell", label: t("settings.tv.action.sell") },
                { value: "close", label: t("settings.tv.action.close") },
              ]}
            />
          )}
          <Segmented
            label={t(k("market"))}
            value={f.market}
            onChange={(market) => set({ market })}
            options={[
              { value: "futures", label: t("filters.market.futures") },
              { value: "spot", label: t("filters.market.spot") },
            ]}
          />
        </div>
        {entry ? (
          <div className="ae-tvb__row">
            <Segmented<LevelKind>
              label={t(k("levels"))}
              value={f.levels}
                onChange={(levels) => set({ levels })}
              options={[
                { value: "pct", label: t(k("levelsPct")) },
                { value: "price", label: t(k("levelsPrice")) },
              ]}
            />
            <TextField label={f.levels === "pct" ? t(k("tpPct")) : t(k("tpPrice"))} value={f.tp} inputMode="decimal" onChange={(e) => set({ tp: e.target.value })} />
            <TextField label={f.levels === "pct" ? t(k("slPct")) : t(k("slPrice"))} value={f.sl} inputMode="decimal" onChange={(e) => set({ sl: e.target.value })} />
            <TextField label={t(k("size"))} value={f.size} inputMode="decimal" placeholder={t(k("sizeEmpty"))} onChange={(e) => set({ size: e.target.value })} />
          </div>
        ) : null}
        <div className="ae-tvb__row">
          <label className="ae-tvb__check">
            <input type="checkbox" checked={f.price} onChange={(e) => set({ price: e.target.checked })} />
            {t(k("price"), PLACEHOLDERS)}
          </label>
          {strategy ? (
            <label className="ae-tvb__check">
              <input type="checkbox" checked={f.alertMessage} onChange={(e) => set({ alertMessage: e.target.checked })} />
              {t(k("alertMessage"), PLACEHOLDERS)}
            </label>
          ) : null}
        </div>
        <div className="ae-tvb__row">
          <TextField
            label={t(k("passphrase"))}
            value={f.passphrase}
            autoComplete="off"
            spellCheck={false}
            hint={t(k("passphraseHint"))}
            onChange={(e) => set({ passphrase: e.target.value })}
          />
          <Segmented<IdKind>
            label={t(k("id"))}
            value={f.id}
            onChange={(id) => set({ id })}
            options={[
              { value: "none", label: t(k("idNone")) },
              { value: "time", label: t(k("idTime")) },
              { value: "custom", label: t(k("idCustom")) },
            ]}
          />
          {f.id === "custom" ? <TextField label={t(k("customId"))} value={f.customId} onChange={(e) => set({ customId: e.target.value })} /> : null}
        </div>
        {built.json ? (
          <div className="ae-tv__once">
            <code className="ae-tg__code ae-tv__tpl" data-testid="tv-built">
              {built.json}
            </code>
            <Button variant="secondary" size="sm" onClick={() => onCopy(built.json!)}>
              {copyLabel}
            </Button>
          </div>
        ) : (
          <p className="ae-sfield__error" role="alert">
            {t(k(`error.${built.error}`))}
          </p>
        )}
        <ul className="ae-setlist">
          {(strategy ? ["strategyMap", "flip", "priceGuard", "idNote"] : ["manualMap", "priceGuard", "idNote"]).map((n) => (
            <li key={n}>{t(k(`note.${n}`), PLACEHOLDERS)}</li>
          ))}
          {strategy && f.alertMessage ? <li>{t(k("note.alertMessage"))}</li> : null}
        </ul>
      </div>
    </Panel>
  );
}
