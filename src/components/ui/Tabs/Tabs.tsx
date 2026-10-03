import { useEffect, useId, useRef, type KeyboardEvent, type ReactNode } from "react";
import "./Tabs.css";

export interface TabDef<T extends string> {
  id: T;
  label: ReactNode;
  /** Marks a tab whose actions touch real money (red label). */
  tone?: "live";
}

interface TabsProps<T extends string> {
  tabs: TabDef<T>[];
  value: T;
  onChange: (id: T) => void;
  label: string;
  children: ReactNode;
}

/**
 * Accessible tabs: Left/Right/Home/End inside the tab list, and
 * Ctrl+Shift+Left/Right from anywhere on the page. The selected tab lives in
 * the URL (?tab=), owned by the caller.
 */
export function Tabs<T extends string>({ tabs, value, onChange, label, children }: TabsProps<T>) {
  const base = useId();
  const listRef = useRef<HTMLDivElement>(null);
  const index = Math.max(0, tabs.findIndex((t) => t.id === value));

  const select = (i: number, focus: boolean) => {
    const next = tabs[(i + tabs.length) % tabs.length];
    onChange(next.id);
    if (focus) {
      requestAnimationFrame(() => {
        listRef.current?.querySelector<HTMLButtonElement>(`[data-tab="${next.id}"]`)?.focus();
      });
    }
  };

  const onKey = (e: KeyboardEvent<HTMLDivElement>) => {
    if (e.key === "ArrowRight") select(index + 1, true);
    else if (e.key === "ArrowLeft") select(index - 1, true);
    else if (e.key === "Home") select(0, true);
    else if (e.key === "End") select(tabs.length - 1, true);
    else return;
    e.preventDefault();
  };

  // Page-wide tab switching.
  const latest = useRef({ index, select });
  latest.current = { index, select };
  useEffect(() => {
    const onDoc = (e: globalThis.KeyboardEvent) => {
      if (!e.ctrlKey || !e.shiftKey) return;
      if (e.key !== "ArrowRight" && e.key !== "ArrowLeft") return;
      e.preventDefault();
      const { index: i, select: sel } = latest.current;
      sel(e.key === "ArrowRight" ? i + 1 : i - 1, false);
    };
    document.addEventListener("keydown", onDoc);
    return () => document.removeEventListener("keydown", onDoc);
  }, []);

  return (
    <div className="ae-tabs">
      <div className="ae-tabs__list" role="tablist" aria-label={label} ref={listRef} onKeyDown={onKey}>
        {tabs.map((tab) => {
          const selected = tab.id === value;
          return (
            <button
              key={tab.id}
              type="button"
              role="tab"
              id={`${base}-${tab.id}`}
              data-tab={tab.id}
              data-tone={tab.tone}
              aria-selected={selected}
              aria-controls={`${base}-panel`}
              tabIndex={selected ? 0 : -1}
              className="ae-tabs__tab"
              onClick={() => onChange(tab.id)}
            >
              {tab.label}
              <span className="ae-tabs__bar" aria-hidden="true" />
            </button>
          );
        })}
      </div>
      <div
        className="ae-tabs__panel"
        role="tabpanel"
        id={`${base}-panel`}
        aria-labelledby={`${base}-${value}`}
      >
        {children}
      </div>
    </div>
  );
}
