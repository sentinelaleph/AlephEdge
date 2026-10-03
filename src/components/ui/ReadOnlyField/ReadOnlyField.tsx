import { useId, type ReactNode } from "react";
import "@/components/ui/TextField/TextField.css";
import "./ReadOnlyField.css";

interface ReadOnlyFieldProps {
  label: string;
  value: ReactNode;
  /** One line under the value (same slot as an input's hint). */
  hint?: ReactNode;
  /** Mono figures for numbers. */
  numeric?: boolean;
}

/**
 * A fixed value inside a form ("Price source: Binance", "Margin mode:
 * Isolated"). Same label and rhythm as an input so the grid stays aligned,
 * but no field chrome: no fill, no border, nothing that invites a click.
 *
 *   <ReadOnlyField label={t("strategy.field.marginMode")} value={t("strategy.field.isolated")} />
 */
export function ReadOnlyField({ label, value, hint, numeric }: ReadOnlyFieldProps) {
  const id = useId();
  return (
    <div className="ae-rofield">
      <span className="ae-field__label" id={id}>
        {label}
      </span>
      <span className="ae-rofield__value" aria-labelledby={id} data-num={numeric || undefined}>
        {value}
      </span>
      {hint ? <p className="ae-field__hint">{hint}</p> : null}
    </div>
  );
}
