import { useEffect, useLayoutEffect, useRef, useState, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { useTranslation } from "react-i18next";
import { useShellSlots } from "@/app/AppShell/shellSlots";
import { Icon } from "@/app/AppShell/icons";
import { Link, recallScroll, rememberScroll, useRouter } from "@/app/router/router";
import { IconButton } from "@/components/ui/Button/Button";
import { OverflowMenu } from "@/components/ui/OverflowMenu/OverflowMenu";
import { pickLevel, type ActionLevel } from "./actionLevel";
import "./PageShell.css";

export interface Crumb {
  label: string;
  /** In-app path; the current page's crumb has none. */
  to?: string;
}

interface PageShellProps {
  title: string;
  /** Parent crumbs, root first. The page title is appended as the last crumb. */
  crumbs?: Crumb[];
  /**
   * Header actions, in the fixed order [secondary][primary][panel toggle].
   * `primary` is the ONE main action of the page (Button variant="primary"
   * size="sm"); `secondary` holds the rest (variant="secondary" size="sm").
   * A page with no action passes neither: the slot stays empty.
   */
  primary?: ReactNode;
  secondary?: ReactNode;
  /** @deprecated Use `primary` / `secondary`. Rendered between them. */
  actions?: ReactNode;
  /** Filters / configuration for this page. */
  left?: ReactNode;
  /** Summaries and readouts for this page. */
  right?: ReactNode;
  /** The wider left panel (backtest configuration). */
  wideLeft?: boolean;
  /** One centered column without side panels. */
  single?: boolean;
  children: ReactNode;
}

/**
 * The Sentinel page shape: left = filters/config, center = workflow (the only
 * scrolling region), right = summaries. The heading and actions are portalled
 * into the global header; the document title follows the page.
 *
 * Narrow windows: the right panel becomes a drawer behind the panel-toggle
 * icon button (tooltip "Summary panel") under 1440px on a three-panel page and
 * under 1280px otherwise; under 1100px the left panel folds behind "Filters".
 * The title is never cut by its actions: when they do not fit beside it, the
 * secondary actions move into a "More" menu, then the primary one too
 * (actionLevel.ts). Under 768px the crumbs hide and the title may wrap.
 *
 * Content headings: the page title lives in the header only. Do not repeat it
 * (or a synonym) as an <h2> at the top of the center column; group content
 * with <Panel title> (framed) or <Section title> (unframed), both 13px/600.
 */
export function PageShell({ title, crumbs, primary, secondary, actions, left, right, wideLeft, single, children }: PageShellProps) {
  const { t } = useTranslation();
  const slots = useShellSlots();
  const { route, kind } = useRouter();
  const headingRef = useRef<HTMLHeadingElement>(null);
  const centerRef = useRef<HTMLDivElement>(null);
  const [summaryOpen, setSummaryOpen] = useState(false);
  const [leftOpen, setLeftOpen] = useState(false);
  const [level, setLevel] = useState<ActionLevel>(0);
  const levelRef = useRef<ActionLevel>(0);
  levelRef.current = level;
  const measureRef = useRef<HTMLDivElement>(null);
  const hasSecondary = !!(secondary || actions);
  const scrollKey = route.path + "?" + route.query.toString();

  useEffect(() => {
    document.title = `${title} - ${t("app.name")}`;
  }, [title, t]);

  // Route change: focus the page heading (screen readers announce the page);
  // restore the center scroll on back/forward, start at the top otherwise.
  useEffect(() => {
    if (kind !== "initial") headingRef.current?.focus({ preventScroll: true });
    const el = centerRef.current;
    if (!el) return;
    const saved = kind === "pop" ? recallScroll(scrollKey) : null;
    el.scrollTop = saved ?? 0;
    // Only a path change moves focus and scroll; `kind` changes with it.
  }, [route.path]);

  useEffect(() => {
    const el = centerRef.current;
    if (!el) return;
    let frame = 0;
    const onScroll = () => {
      cancelAnimationFrame(frame);
      frame = requestAnimationFrame(() => rememberScroll(scrollKey, el.scrollTop));
    };
    el.addEventListener("scroll", onScroll, { passive: true });
    return () => {
      cancelAnimationFrame(frame);
      el.removeEventListener("scroll", onScroll);
    };
  }, [scrollKey]);

  // Esc closes the summary drawer.
  useEffect(() => {
    if (!summaryOpen) return;
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && setSummaryOpen(false);
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [summaryOpen]);

  // Fit the actions beside the title: measure the hidden natural-width copies
  // against the room the heading and the action bar share in the header.
  useLayoutEffect(() => {
    const head = slots.heading;
    const bar = slots.actions;
    const m = measureRef.current;
    if (!head || !bar || !m) return;
    const width = (part: string) => m.querySelector<HTMLElement>(`[data-part="${part}"]`)?.offsetWidth ?? 0;
    const measure = () => {
      const parts = {
        secondary: hasSecondary ? width("secondary") : 0,
        primary: primary ? width("primary") : 0,
        more: width("more"),
        gap: parseFloat(getComputedStyle(m).columnGap) || 8,
      };
      const room = head.clientWidth + bar.clientWidth;
      // The panel toggle stays whatever the level (hidden on a wide window).
      const toggle = bar.querySelector<HTMLElement>(".ae-pageactions__summary")?.offsetWidth ?? 0;
      const fixed = toggle > 0 ? toggle + parts.gap : 0;
      const next = pickLevel(room, fixed, width("title"), parts);
      if (next !== levelRef.current) setLevel(next);
    };
    measure();
    if (typeof ResizeObserver === "undefined") return;
    const ro = new ResizeObserver(measure);
    if (head.parentElement) ro.observe(head.parentElement);
    ro.observe(m);
    return () => ro.disconnect();
  });

  const layout = single ? "single" : left && right ? "three" : left ? "left" : right ? "right" : "center";

  const heading = (
    <div className="ae-pagehead">
      {crumbs && crumbs.length > 0 ? (
        <nav className="ae-pagehead__crumbs" aria-label={t("nav.breadcrumb")}>
          <ol>
            {crumbs.map((c) => (
              <li key={c.label}>{c.to ? <Link to={c.to}>{c.label}</Link> : <span>{c.label}</span>}</li>
            ))}
          </ol>
        </nav>
      ) : null}
      <h1 ref={headingRef} id="ae-page-title" className="ae-pagehead__title" tabIndex={-1}>
        {title}
      </h1>
    </div>
  );

  const toggleShown = !!right && !single;
  const actionBar = (
    <div className="ae-pageactions" data-layout={layout} data-level={level}>
      {level === 0 ? (
        <>
          {secondary}
          {actions}
          {primary}
        </>
      ) : (
        <>
          <OverflowMenu>
            {level === 2 ? primary : null}
            {secondary}
            {actions}
          </OverflowMenu>
          {level === 1 ? primary : null}
        </>
      )}
      {/* Natural widths for the fit, never seen or reached. */}
      <div className="ae-pageactions__measure" ref={measureRef} aria-hidden="true" {...{ inert: "" }}>
        <span data-part="title" className="ae-pagehead__title">
          {title}
        </span>
        {hasSecondary ? (
          <span data-part="secondary" className="ae-pageactions__group">
            {secondary}
            {actions}
          </span>
        ) : null}
        {primary ? <span data-part="primary">{primary}</span> : null}
        <span data-part="more" className="ae-iconbtn ae-iconbtn--sm" />
      </div>
      {toggleShown ? (
        <span className="ae-pageactions__summary">
          <IconButton
            label={t("nav.summaryPanel")}
            icon={<Icon name="panelRight" />}
            active={summaryOpen}
            aria-expanded={summaryOpen}
            aria-controls="ae-page-right"
            tooltipAlign="end"
            onClick={() => setSummaryOpen((v) => !v)}
          />
        </span>
      ) : null}
    </div>
  );

  return (
    <div
      className="ae-page"
      data-layout={layout}
      data-wide-left={wideLeft || undefined}
      data-summary-open={summaryOpen || undefined}
      data-left-open={leftOpen || undefined}
    >
      {slots.heading ? createPortal(heading, slots.heading) : <div className="ae-page__inlinehead">{heading}</div>}
      {slots.actions ? createPortal(actionBar, slots.actions) : <div className="ae-page__inlineactions">{actionBar}</div>}

      {left && !single ? (
        <aside className="ae-page__left" aria-label={t("nav.filters")}>
          <button
            type="button"
            className="ae-page__leftfold"
            aria-expanded={leftOpen}
            onClick={() => setLeftOpen((v) => !v)}
          >
            {t("nav.filters")}
          </button>
          <div className="ae-page__leftbody">{left}</div>
        </aside>
      ) : null}

      <div className="ae-page__center" ref={centerRef} id="ae-main-content" tabIndex={-1}>
        <div className="ae-page__centerinner">{children}</div>
      </div>

      {right && !single ? (
        <>
          <button
            type="button"
            className="ae-page__scrim"
            aria-label={t("nav.closeSummary")}
            tabIndex={-1}
            onClick={() => setSummaryOpen(false)}
          />
          <aside className="ae-page__right" id="ae-page-right" aria-label={t("nav.summary")}>
            {right}
          </aside>
        </>
      ) : null}
    </div>
  );
}
