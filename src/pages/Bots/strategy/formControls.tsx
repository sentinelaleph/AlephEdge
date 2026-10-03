import { useEffect, useId, useRef, useState, type ReactNode } from "react";
import { SegmentedControl } from "@/components/ui/SegmentedControl/SegmentedControl";
import { formatDecimalInput, parseDecimal } from "@/lib/decimal";

interface FieldShellProps {
  label: string;
  htmlFor?: string;
  hint?: ReactNode;
  error?: string | null;
  children: ReactNode;
}

/** Label, control, then one hint or error line. */
export function FieldShell({ label, htmlFor, hint, error, children }: FieldShellProps) {
  return (
    <div className="ae-sfield" data-invalid={error ? true : undefined}>
      <label className="ae-field__label" htmlFor={htmlFor}>
        {label}
      </label>
      {children}
      {error ? (
        <p className="ae-sfield__error" role="alert">
          {error}
        </p>
      ) : hint ? (
        <p className="ae-field__hint">{hint}</p>
      ) : null}
    </div>
  );
}

/** Shared with every other number field (src/lib/decimal). */
const parse = parseDecimal;

interface NumFieldProps {
  label: string;
  value: number;
  onChange: (v: number) => void;
  /** Reports a local parse failure (not a number / not an integer / out of the wire type). */
  onInvalid: (bad: boolean) => void;
  unit?: string;
  integer?: boolean;
  /** Wire-type ceiling (u8 = 255 …); Rust owns the real bounds. */
  wireMax?: number;
  step?: number;
  disabled?: boolean;
  hint?: ReactNode;
  error?: string | null;
  invalidText: string;
}

/**
 * A number input that keeps what the user types (so "1." or "" can exist
 * mid-edit) and only reports parsed, finite values upward.
 */
export function NumField({ label, value, onChange, onInvalid, unit, integer, wireMax, step, disabled, hint, error, invalidText }: NumFieldProps) {
  const id = useId();
  const [text, setText] = useState(() => formatDecimalInput(value));
  const [bad, setBad] = useState(false);
  const report = useRef(onInvalid);
  report.current = onInvalid;

  // A field that leaves the form (its switch turned off) takes its error along.
  useEffect(() => () => report.current(false), []);

  // An outside change (preset, defaults, market switch) replaces the text.
  useEffect(() => {
    if (parse(text) !== value && Number.isFinite(value)) {
      setText(formatDecimalInput(value));
      setBad(false);
      report.current(false);
    }
    // `text` is deliberately not a dependency: typing must not be overwritten.
  }, [value]);

  const change = (next: string) => {
    setText(next);
    const n = parse(next);
    const ok =
      Number.isFinite(n) && n >= 0 && (!integer || Number.isInteger(n)) && (wireMax === undefined || n <= wireMax);
    setBad(!ok);
    report.current(!ok);
    if (ok) onChange(n);
  };

  return (
    <FieldShell label={label} htmlFor={id} hint={hint} error={bad ? invalidText : error}>
      <span className="ae-sfield__input">
        <input
          id={id}
          className="ae-field__input mono"
          inputMode="decimal"
          value={text}
          step={step}
          disabled={disabled}
          aria-invalid={bad || !!error || undefined}
          onChange={(e) => change(e.target.value)}
        />
        {unit ? <span className="ae-sfield__unit">{unit}</span> : null}
      </span>
    </FieldShell>
  );
}

export interface SegOption<T extends string> {
  value: T;
  label: string;
  disabled?: boolean;
  /** Why the option is disabled (shown as its title). */
  reason?: string;
}

interface SegmentedProps<T extends string> {
  label: string;
  value: T;
  options: SegOption<T>[];
  onChange: (v: T) => void;
  disabled?: boolean;
  hint?: ReactNode;
  error?: string | null;
}

/** A labelled radio group drawn as the shared SegmentedControl. */
export function Segmented<T extends string>({ label, value, options, onChange, disabled, hint, error }: SegmentedProps<T>) {
  return (
    <FieldShell label={label} hint={hint} error={error}>
      <SegmentedControl
        label={label}
        value={value}
        fill
        wrap
        disabled={disabled}
        options={options.map((o) => ({ value: o.value, label: o.label, disabled: o.disabled, reason: o.reason }))}
        onChange={onChange}
      />
    </FieldShell>
  );
}

interface CheckRowProps {
  label: string;
  checked: boolean;
  onChange: (v: boolean) => void;
  disabled?: boolean;
  hint?: ReactNode;
  error?: string | null;
}

/** A labelled checkbox row (optional settings switch on/off). */
export function CheckRow({ label, checked, onChange, disabled, hint, error }: CheckRowProps) {
  const id = useId();
  return (
    <div className="ae-scheck" data-invalid={error ? true : undefined}>
      <label htmlFor={id}>
        <input id={id} type="checkbox" checked={checked} disabled={disabled} onChange={(e) => onChange(e.target.checked)} />
        <span>{label}</span>
      </label>
      {error ? (
        <p className="ae-sfield__error" role="alert">
          {error}
        </p>
      ) : hint ? (
        <p className="ae-field__hint">{hint}</p>
      ) : null}
    </div>
  );
}
