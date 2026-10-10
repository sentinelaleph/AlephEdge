import { useCallback, useEffect, useRef, useState, type KeyboardEvent, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { useDeskContext } from "@/app/DeskProvider";
import { Link, useRouter } from "@/app/router/router";
import { Dot, type DotTone } from "@/components/ui/Chip/Chip";
import { EdgeMark } from "@/components/Shell/EdgeMark";
import { useTheme, type ThemeMode } from "@/theme/ThemeProvider";
import { Icon, type IconName } from "./icons";
import { activeNavItem, groupOf, NAV_GROUPS, type NavGroupId, type NavItemId } from "./navModel";
import { streamLabelKey, streamTone } from "@/lib/ipc/signal/streamStatus";
import { useStrategyLive } from "@/pages/Bots/strategy/useStrategyLive";

const GROUPS_KEY = "aleph-edge-nav-groups";

function readGroups(): Partial<Record<NavGroupId, boolean>> {
  try {
    const raw = localStorage.getItem(GROUPS_KEY);
    return raw ? (JSON.parse(raw) as Partial<Record<NavGroupId, boolean>>) : {};
  } catch {
    return {};
  }
}

export type NavMode = "expanded" | "rail" | "drawer";

interface SidebarProps {
  mode: NavMode;
  /** Rail/expanded toggle (footer button, Ctrl+B). */
  onToggle: () => void;
  /** Drawer mode only: close after a navigation or Esc. */
  onClose?: () => void;
}

interface Badge {
  count?: number;
  /** What the count counts ("Running 2", "Open 3"): tooltip and screen-reader text. */
  countLabel?: string;
  dots?: { tone: DotTone; label: string }[];
  chips?: { tone: "paper" | "live" | "real"; label: string; title?: string }[];
}

/**
 * The page index: grouped destinations with live badges from desk state,
 * keyboard-navigable as one roving-tabindex list (Up/Down move, Left/Right
 * fold a group, Enter opens).
 */
export function Sidebar({ mode, onToggle, onClose }: SidebarProps) {
  const { t } = useTranslation();
  const ctx = useDeskContext();
  const { route, from } = useRouter();
  const theme = useTheme();
  const active = activeNavItem(route, from, (id) => ctx.strategy.bots?.find((b) => b.id === id)?.kind ?? null);
  const activeGroup = groupOf(active);
  const [openGroups, setOpenGroups] = useState(readGroups);
  const [focusKey, setFocusKey] = useState<string>(active ?? "dashboard");
  const navRef = useRef<HTMLElement>(null);
  const rail = mode === "rail";

  // Following a link into a folded group unfolds it.
  useEffect(() => {
    if (activeGroup && openGroups[activeGroup] === false) {
      setOpenGroups((g) => ({ ...g, [activeGroup]: true }));
    }
    if (active) setFocusKey(active);
    // Only a change of the active item drives this, not the stored folds.
  }, [active, activeGroup]);

  // On a short window the list scrolls: keep the current page's item in view
  // (Ctrl+9 to Risk & safety lands below the fold at 1280x800).
  useEffect(() => {
    if (!active) return;
    const el = navRef.current?.querySelector<HTMLElement>(`.ae-sidebar__scroll [data-nav-key="${active}"]`);
    el?.scrollIntoView?.({ block: "nearest" });
  }, [active]);

  const setGroup = useCallback((id: NavGroupId, open: boolean) => {
    setOpenGroups((g) => {
      const next = { ...g, [id]: open };
      try {
        localStorage.setItem(GROUPS_KEY, JSON.stringify(next));
      } catch {
        /* preference not remembered; the toggle still works */
      }
      return next;
    });
  }, []);

  const isOpen = (id: NavGroupId) => rail || id === activeGroup || openGroups[id] !== false;

  const badges = useBadges();

  const onKeyDown = (e: KeyboardEvent<HTMLElement>) => {
    const items = Array.from(navRef.current?.querySelectorAll<HTMLElement>("[data-nav-key]") ?? []);
    const i = items.findIndex((el) => el === document.activeElement);
    if (i < 0) return;
    const focusAt = (j: number) => {
      const el = items[(j + items.length) % items.length];
      setFocusKey(el.dataset.navKey ?? "");
      el.focus();
    };
    const el = items[i];
    const group = el.dataset.navGroup as NavGroupId | undefined;
    if (e.key === "ArrowDown") focusAt(i + 1);
    else if (e.key === "ArrowUp") focusAt(i - 1);
    else if (e.key === "Home") focusAt(0);
    else if (e.key === "End") focusAt(items.length - 1);
    else if (e.key === "ArrowLeft" && group && !rail) {
      const g = NAV_GROUPS.find((x) => x.id === group);
      if (g?.collapsible && group !== activeGroup) setGroup(group, false);
      const head = items.find((x) => x.dataset.navKey === `group:${group}`);
      if (head) {
        setFocusKey(head.dataset.navKey ?? "");
        head.focus();
      }
    } else if (e.key === "ArrowRight" && group && !rail) setGroup(group, true);
    else if (e.key === "Escape" && onClose) onClose();
    else return;
    e.preventDefault();
  };

  // The roving stop must land on a rendered item: group headers and the
  // collapse button are absent in rail / drawer mode.
  const stopKey =
    (rail && focusKey.startsWith("group:")) || (mode === "drawer" && focusKey === "collapse")
      ? (active ?? "dashboard")
      : focusKey;
  const tabFor = (key: string) => (key === stopKey ? 0 : -1);
  const afterNav = () => onClose?.();

  return (
    <nav
      ref={navRef}
      className="ae-sidebar"
      data-mode={mode}
      aria-label={t("nav.label")}
      onKeyDown={onKeyDown}
    >
      <div className="ae-sidebar__brand">
        <span className="ae-sidebar__mark" aria-hidden="true">
          <EdgeMark />
        </span>
        {!rail ? (
          <>
            <span className="ae-sidebar__name">{t("app.name")}</span>
            <span className="ae-sidebar__beta">{t("app.betaBadge")}</span>
          </>
        ) : null}
      </div>

      <div className="ae-sidebar__scroll">
        {NAV_GROUPS.map((g) => {
          const open = isOpen(g.id);
          return (
            <div key={g.id} className="ae-sidebar__group" role="group" aria-labelledby={`ae-navg-${g.id}`}>
              {rail ? (
                <span id={`ae-navg-${g.id}`} className="ae-sidebar__railsep">
                  <span className="ae-visually-hidden">{t(g.labelKey)}</span>
                </span>
              ) : g.collapsible ? (
                <button
                  type="button"
                  id={`ae-navg-${g.id}`}
                  className="ae-sidebar__grouphead"
                  aria-expanded={open}
                  aria-disabled={g.id === activeGroup || undefined}
                  data-nav-key={`group:${g.id}`}
                  data-nav-group={g.id}
                  tabIndex={tabFor(`group:${g.id}`)}
                  onClick={() => g.id !== activeGroup && setGroup(g.id, !open)}
                >
                  <span>{t(g.labelKey)}</span>
                  <span className="ae-sidebar__chev" data-open={open} aria-hidden="true">
                    <Icon name="chevron" size={12} />
                  </span>
                </button>
              ) : (
                <span id={`ae-navg-${g.id}`} className="ae-sidebar__grouphead ae-sidebar__grouphead--static">
                  {t(g.labelKey)}
                </span>
              )}
              {open ? (
                <ul className="ae-sidebar__items">
                  {g.items.map((item) => (
                    <li key={item.id}>
                      <NavLink
                        id={item.id}
                        to={item.path}
                        icon={item.icon}
                        label={t(item.labelKey)}
                        active={active === item.id}
                        rail={rail}
                        group={g.id}
                        tabIndex={tabFor(item.id)}
                        badge={badges[item.id]}
                        onNavigate={afterNav}
                      />
                    </li>
                  ))}
                </ul>
              ) : null}
              {g.id === "bots" && open ? (
                <Link
                  to="/bots/new"
                  className="ae-sidebar__new"
                  data-nav-key="botNew"
                  data-nav-group="bots"
                  data-tip={rail ? t("nav.newBot") : undefined}
                  aria-current={active === "botNew" ? "page" : undefined}
                  aria-keyshortcuts="N"
                  tabIndex={tabFor("botNew")}
                  onClick={afterNav}
                >
                  <Icon name="plus" />
                  {!rail ? (
                    <>
                      <span className="ae-sidebar__label">{t("nav.newBot")}</span>
                      <kbd className="ae-sidebar__kbd">N</kbd>
                    </>
                  ) : (
                    <span className="ae-visually-hidden">{t("nav.newBot")}</span>
                  )}
                </Link>
              ) : null}
            </div>
          );
        })}
      </div>

      <div className="ae-sidebar__footer">
        <NavLink
          id="settings"
          to="/settings"
          icon="settings"
          label={t("nav.settings")}
          active={active === "settings"}
          rail={rail}
          badge={badges.settings}
          tabIndex={tabFor("settings")}
          onNavigate={afterNav}
        />
        <NavLink
          id="account"
          to="/account"
          label={t("nav.account")}
          initial={(ctx.membership.view.email ?? "?").slice(0, 1).toUpperCase()}
          active={active === "account"}
          rail={rail}
          tabIndex={tabFor("account")}
          onNavigate={afterNav}
        />
        <div className="ae-sidebar__tools">
          <button
            type="button"
            className="ae-sidebar__tool"
            data-nav-key="theme"
            tabIndex={tabFor("theme")}
            onClick={theme.cycle}
            aria-label={t("nav.themeCycle", { mode: t(`theme.${theme.mode}`) })}
            data-tip={t("nav.themeCycle", { mode: t(`theme.${theme.mode}`) })}
          >
            <Icon name={THEME_ICON[theme.mode]} />
          </button>
          {mode !== "drawer" ? (
            <button
              type="button"
              className="ae-sidebar__tool"
              data-nav-key="collapse"
              tabIndex={tabFor("collapse")}
              onClick={onToggle}
              aria-label={rail ? t("nav.expand") : t("nav.collapse")}
              aria-keyshortcuts="Control+B"
              data-tip={rail ? t("nav.expand") : t("nav.collapse")}
            >
              <Icon name={rail ? "expand" : "collapse"} />
            </button>
          ) : null}
        </div>
      </div>
    </nav>
  );
}

const THEME_ICON: Record<ThemeMode, IconName> = {
  light: "sun",
  dark: "moon",
  contrast: "contrast",
  system: "system",
};

interface NavLinkProps {
  id: NavItemId;
  to: string;
  icon?: IconName;
  /** A letter avatar instead of an icon (account). */
  initial?: string;
  label: string;
  active: boolean;
  rail: boolean;
  group?: NavGroupId;
  tabIndex: number;
  badge?: Badge;
  onNavigate: () => void;
}

/** Every badge as words, for the rail tooltip and screen readers. */
export function badgeText(badge?: Badge): string {
  if (!badge) return "";
  const parts: string[] = [];
  if (badge.count && badge.count > 0) parts.push(badge.countLabel ?? String(badge.count));
  badge.dots?.forEach((d) => parts.push(d.label));
  badge.chips?.forEach((c) => parts.push(c.title ?? c.label));
  return parts.join(" · ");
}

/** The rail dot's colour: the most serious dot, else accent for a count. */
function railTone(badge?: Badge): DotTone | "accent" | null {
  const tones = badge?.dots?.map((d) => d.tone) ?? [];
  for (const tone of ["danger", "warn", "success", "muted"] as const) if (tones.includes(tone)) return tone;
  if (badge?.chips?.some((c) => c.tone !== "paper")) return "danger";
  return (badge?.count ?? 0) > 0 ? "accent" : null;
}

function NavLink({ id, to, icon, initial, label, active, rail, group, tabIndex, badge, onNavigate }: NavLinkProps) {
  const extra = badgeText(badge);
  const full = extra ? `${label} · ${extra}` : label;
  const dot = rail ? railTone(badge) : null;
  return (
    <Link
      to={to}
      className="ae-navitem"
      aria-current={active ? "page" : undefined}
      data-nav-key={id}
      data-nav-group={group}
      data-tip={rail ? full : undefined}
      tabIndex={tabIndex}
      onClick={onNavigate}
    >
      <span className="ae-navitem__icon">
        {initial ? <span className="ae-navitem__initial">{initial}</span> : icon ? <Icon name={icon} /> : null}
        {dot ? <span className="ae-navitem__raildot" data-tone={dot} aria-hidden="true" /> : null}
      </span>
      {rail ? (
        <span className="ae-visually-hidden">{full}</span>
      ) : (
        <>
          <span className="ae-sidebar__label" title={label}>
            {label}
          </span>
          <BadgeView badge={badge} />
        </>
      )}
    </Link>
  );
}

function BadgeView({ badge }: { badge?: Badge }): ReactNode {
  if (!badge) return null;
  return (
    <span className="ae-navitem__badges">
      {badge.dots?.map((d) => <Dot key={d.label} tone={d.tone} label={d.label} />)}
      {badge.chips?.map((c) => (
        <span key={c.label} className="ae-navitem__chip" data-tone={c.tone} title={c.title}>
          {c.label}
        </span>
      ))}
      {badge.count && badge.count > 0 ? (
        <span className="ae-navitem__count tabular" title={badge.countLabel}>
          <span aria-hidden="true">{badge.count}</span>
          <span className="ae-visually-hidden">{badge.countLabel ?? badge.count}</span>
        </span>
      ) : null}
    </span>
  );
}

/** Live badges from desk state. Only real counts; nothing is shown while unknown. */
function useBadges(): Partial<Record<NavItemId, Badge>> {
  const { t } = useTranslation();
  const { desk, feed, vault, strategy } = useDeskContext();
  const strategyLive = useStrategyLive();
  if (!desk.loaded) return {};
  const s = desk.status;
  const running = [s.futuresRunning, s.spotRunning, s.pumpRunning].filter(Boolean).length;
  const paperOpen = s.openPositions.filter((p) => !p.live).length;
  const realOpen = s.openPositions.some((p) => p.live) || strategyLive.views.some((v) => v.realQty !== 0);
  const anyLive = [s.futures, s.spot, s.pump].some((c) => c?.live === true) || strategyLive.anyLive;
  // Strategy bots arming or holding a cycle (paper only).
  const strategyActive = (kind: "dca" | "grid") =>
    strategy.bots?.filter((b) => b.kind === kind && (b.acceptingNewCycles || b.openCycle !== null)).length ?? 0;
  const streamDot: DotTone = streamTone(feed.health);

  const riskDots: Badge["dots"] = [];
  if (s.killSwitchTripped) riskDots.push({ tone: "danger", label: t("nav.badge.killSwitch") });
  if (s.btcRegime !== "normal") riskDots.push({ tone: "warn", label: t("nav.badge.regime") });

  const runningBadge = (count: number): Badge => ({ count, countLabel: t("nav.badge.running", { count }) });
  const openCount = paperOpen + (strategy.bots?.filter((b) => b.openCycle !== null).length ?? 0);

  // The mode (Paper) is shown once, in the header: no per-item Paper chips.
  return {
    bots: runningBadge(running + strategyActive("dca") + strategyActive("grid")),
    signalBots: runningBadge(running),
    dcaBots: runningBadge(strategyActive("dca")),
    gridBots: runningBadge(strategyActive("grid")),
    signals: feed.loaded
      ? { dots: [{ tone: streamDot, label: t(streamLabelKey(feed.health)) }] }
      : undefined,
    positions: {
      count: openCount,
      countLabel: t("nav.badge.open", { count: openCount }),
      chips: realOpen ? [{ tone: "real", label: "R", title: t("nav.badge.real") }] : undefined,
    },
    // A missing key matters only where a key can trade: the paper build needs none.
    settings:
      s.liveTradingEnabled && vault.credentials.length === 0
        ? { dots: [{ tone: "warn", label: t("nav.badge.noKey") }] }
        : undefined,
    risk: {
      dots: riskDots,
      chips: anyLive ? [{ tone: "live", label: "LIVE", title: t("nav.badge.live") }] : undefined,
    },
  };
}
