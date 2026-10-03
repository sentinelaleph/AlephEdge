import { useId } from "react";
import { useTranslation } from "react-i18next";
import "./FilterPanel.css";

export interface FilterOption {
  value: string;
  label: string;
  /** How many rows carry this value (shown at the row's right edge). */
  count?: number;
}

interface FilterGroupProps {
  label: string;
  options: FilterOption[];
  /** Selected values; empty = no filter. */
  selected: string[];
  onChange: (next: string[]) => void;
  /** One value at a time (radio behaviour) instead of a set. */
  single?: boolean;
}

/**
 * A labelled list of filter rows: a check (or a radio dot for `single`),
 * the label, and its count aligned on the right. Rows rather than wrapping
 * chips, so labels of any length line up and counts read as a column. A
 * group with a selection offers Clear next to its title. Every option
 * filters something real.
 */
export function FilterGroup({ label, options, selected, onChange, single }: FilterGroupProps) {
  const { t } = useTranslation();
  const id = useId();
  const toggle = (v: string) => {
    if (single) onChange(selected.includes(v) ? [] : [v]);
    else onChange(selected.includes(v) ? selected.filter((x) => x !== v) : [...selected, v]);
  };
  return (
    <div className="ae-filter" role="group" aria-labelledby={id}>
      <div className="ae-filter__head">
        <span id={id} className="ae-filter__label">
          {label}
        </span>
        {selected.length > 0 ? (
          <button type="button" className="ae-filter__clear" onClick={() => onChange([])}>
            {t("filters.clear")}
          </button>
        ) : null}
      </div>
      <ul className="ae-filter__list">
        {options.map((o) => {
          const on = selected.includes(o.value);
          return (
            <li key={o.value}>
              <button
                type="button"
                className="ae-filter__opt"
                data-kind={single ? "radio" : "check"}
                data-empty={o.count === 0 ? "true" : undefined}
                aria-pressed={on}
                onClick={() => toggle(o.value)}
              >
                <span className="ae-filter__mark" aria-hidden="true" />
                <span className="ae-filter__text">{o.label}</span>
                {o.count !== undefined ? <span className="ae-filter__count tabular">{o.count}</span> : null}
              </button>
            </li>
          );
        })}
      </ul>
    </div>
  );
}

interface SearchFieldProps {
  label: string;
  value: string;
  onChange: (v: string) => void;
  placeholder?: string;
}

/** The page's search box; "/" focuses it from anywhere. */
export function SearchField({ label, value, onChange, placeholder }: SearchFieldProps) {
  const { t } = useTranslation();
  const id = useId();
  return (
    <div className="ae-filter">
      <label htmlFor={id} className="ae-filter__label">
        {label}
      </label>
      <div className="ae-filter__searchbox">
        <svg className="ae-filter__searchicon" viewBox="0 0 16 16" width="14" height="14" aria-hidden="true">
          <circle cx="7" cy="7" r="4.5" fill="none" stroke="currentColor" strokeWidth="1.5" />
          <path d="M10.5 10.5 14 14" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" />
        </svg>
        <input
          id={id}
          className="ae-filter__search"
          type="search"
          value={value}
          placeholder={placeholder ?? t("filters.searchPlaceholder")}
          data-page-search
          aria-keyshortcuts="/"
          onChange={(e) => onChange(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Escape" && value) {
              e.stopPropagation();
              onChange("");
            }
          }}
          title={t("filters.searchShortcut")}
        />
        {!value ? (
          <kbd className="ae-filter__kbd" aria-hidden="true">
            /
          </kbd>
        ) : null}
      </div>
    </div>
  );
}

/** Splits a comma list from the URL into values. */
export function listParam(v: string | null): string[] {
  return v ? v.split(",").filter(Boolean) : [];
}
