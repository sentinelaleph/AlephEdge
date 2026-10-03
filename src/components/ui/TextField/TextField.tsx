import { forwardRef, useId, type InputHTMLAttributes, type ReactNode } from "react";
import { InfoTip } from "@/components/ui/InfoTip/InfoTip";
import "./TextField.css";

interface TextFieldProps extends InputHTMLAttributes<HTMLInputElement> {
  label: string;
  /** Optional helper line under the field. */
  hint?: ReactNode;
  /** The rule behind the field, in an InfoTip next to the label (not around the whole field). */
  info?: ReactNode;
}

/**
 * Labeled text input. The focus ring is a box-shadow transition (never a layout
 * shift), matching the app's calm, no-jump motion contract.
 *
 *   <TextField label={t("risk.dailyLoss")} info={t("risk.dailyLossHint", { cap })} hint={…} … />
 */
export const TextField = forwardRef<HTMLInputElement, TextFieldProps>(
  ({ label, hint, info, id, className, ...rest }, ref) => {
    const autoId = useId();
    const fieldId = id ?? autoId;
    return (
      <div className={`ae-field${className ? ` ${className}` : ""}`}>
        {info ? (
          <span className="ae-field__labelrow">
            <label className="ae-field__label" htmlFor={fieldId}>
              {label}
            </label>
            <InfoTip label={label} content={info} size="sm" placement="top" align="start" />
          </span>
        ) : (
          <label className="ae-field__label" htmlFor={fieldId}>
            {label}
          </label>
        )}
        <input ref={ref} id={fieldId} className="ae-field__input" {...rest} />
        {hint ? <p className="ae-field__hint">{hint}</p> : null}
      </div>
    );
  },
);
TextField.displayName = "TextField";
