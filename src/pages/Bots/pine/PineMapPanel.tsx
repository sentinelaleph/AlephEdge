import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Panel } from "@/components/ui/Panel/Panel";
import { parseDecimal } from "@/lib/decimal";
import type { PineAction, PineTriggerInfo } from "@/lib/ipc/pine/pine";
import { FieldShell } from "@/pages/Bots/strategy/formControls";
import { inputProblem, inputValue, SOURCE_NAMES, type PineInputInfo } from "./pineInputs";
import { triggerLabel } from "./pineText";

const ACTIONS: PineAction[] = ["buy", "sell", "close", "ignore"];

/** One input of the script as a form field; an unusable value shows its reason (the page blocks Save). */
function InputField({ info, value, onChange }: { info: PineInputInfo; value: unknown; onChange: (v: unknown) => void }) {
  const { t } = useTranslation();
  const id = `pine-in-${info.id}`;
  const [text, setText] = useState(() => String(value));
  const problem = inputProblem(info, value);
  const label = info.title.trim() || t("botsPine.inputN", { n: info.id + 1 });
  const hint = [info.tooltip, t("botsPine.inputDefault", { value: String(info.defval) })].filter(Boolean).join(" · ");
  const error = problem ? t(`botsPine.inputError.${problem.key}`, problem.params) : null;

  if (info.kind === "bool") {
    return (
      <FieldShell label={label} htmlFor={id} hint={hint}>
        <input id={id} type="checkbox" checked={value === true} onChange={(e) => onChange(e.target.checked)} />
      </FieldShell>
    );
  }
  if (info.kind === "timeframe") {
    return (
      <FieldShell label={label} htmlFor={id} hint={t("botsPine.inputTimeframe")}>
        <input id={id} className="ae-field__input mono" value={String(info.defval) || "—"} disabled readOnly />
      </FieldShell>
    );
  }
  const choices = info.kind === "source" ? [...SOURCE_NAMES] : info.options;
  if (choices) {
    return (
      <FieldShell label={label} htmlFor={id} hint={hint} error={error}>
        <select
          id={id}
          className="ae-field__input"
          value={String(value)}
          onChange={(e) => onChange(choices.find((c) => String(c) === e.target.value) ?? e.target.value)}
        >
          {choices.map((c) => (
            <option key={String(c)} value={String(c)}>
              {String(c)}
            </option>
          ))}
        </select>
      </FieldShell>
    );
  }
  const numeric = info.kind === "int" || info.kind === "float";
  return (
    <FieldShell label={label} htmlFor={id} hint={hint} error={error}>
      <input
        id={id}
        className="ae-field__input mono"
        inputMode={numeric ? "decimal" : undefined}
        value={numeric ? text : String(value)}
        aria-invalid={error ? true : undefined}
        onChange={(e) => {
          const raw = e.target.value;
          setText(raw);
          const next = numeric ? (raw.trim() === "" ? Number.NaN : parseDecimal(raw)) : raw;
          onChange(next);
        }}
      />
    </FieldShell>
  );
}

interface Props {
  inputs: PineInputInfo[];
  overrides: Record<string, unknown>;
  triggers: PineTriggerInfo[];
  map: Record<string, PineAction>;
  onInput: (id: number, v: unknown) => void;
  onMap: (id: string, a: PineAction) => void;
  /** Why Save is off when it is about the trigger map, said where the map is. */
  mapError?: string | null;
}

/**
 * The compiled script's inputs (overrides keyed by input id; the engine
 * refuses a value outside the declaration, so the form checks it first) and
 * its triggers, each mapped to Buy / Sell / Close / Ignore (default Ignore).
 */
export function PineMapPanel({ inputs, overrides, triggers, map, onInput, onMap, mapError }: Props) {
  const { t } = useTranslation();
  const k = (x: string) => `botsPine.${x}`;
  return (
    <>
      <Panel title={t(k("inputs"))}>
        {inputs.length === 0 ? (
          <p className="ae-muted">{t(k("inputsNone"))}</p>
        ) : (
          <div className="ae-sform__grid">
            {inputs.map((info) => (
              <InputField
                key={info.id}
                info={info}
                value={inputValue(info, overrides)}
                onChange={(v) => onInput(info.id, v)}
              />
            ))}
          </div>
        )}
      </Panel>
      <Panel title={t(k("triggers"))}>
        {mapError ? (
          <p className="ae-banner" data-tone="warn" role="status" data-testid="pine-map-error">
            {mapError}
          </p>
        ) : null}
        {triggers.length === 0 ? (
          <p className="ae-muted">{t(k("triggersNone"))}</p>
        ) : (
          <div className="ae-sform__grid">
            {triggers.map((tr) => (
              <FieldShell key={tr.id} label={triggerLabel(t, tr)} htmlFor={`pine-tr-${tr.id}`} hint={t(k("triggerLine"), { line: tr.line, kind: t(k(`triggerKind.${tr.kind}`)) })}>
                <select id={`pine-tr-${tr.id}`} className="ae-field__input" value={map[tr.id] ?? "ignore"} onChange={(e) => onMap(tr.id, e.target.value as PineAction)}>
                  {ACTIONS.map((a) => (
                    <option key={a} value={a}>
                      {t(k(`action.${a}`))}
                    </option>
                  ))}
                </select>
              </FieldShell>
            ))}
          </div>
        )}
      </Panel>
    </>
  );
}
