/**
 * In-house typed hash router (no dependency). Navigation goes through
 * history.pushState with a hash URL, so the window's back/forward (Alt+Left /
 * Alt+Right, mouse back button) walk the same history the app writes.
 *
 * The router is mounted only after the membership and vault gates pass. The
 * hash a user arrived with is captured once at module load and replayed after
 * the gates, so a deep link survives sign-in and unlock.
 */

import {
  useCallback,
  useEffect,
  useSyncExternalStore,
  type AnchorHTMLAttributes,
  type MouseEvent,
} from "react";
import { DEFAULT_PATH, parseHash, type ParsedRoute } from "./routes";

/** History entry state the router writes. `from` is the previous path. */
interface HistoryState {
  aeFrom?: string;
}

export interface RouterSnapshot {
  route: ParsedRoute;
  /** Path of the page the user came from (in-app navigation only). */
  from: string | null;
  /** How the current entry was reached: "pop" restores scroll, "push" resets it. */
  kind: "push" | "pop" | "initial";
}

/** The hash present when the app first loaded (before any gate). */
const INITIAL_HASH = typeof window !== "undefined" ? window.location.hash : "";

function read(kind: RouterSnapshot["kind"]): RouterSnapshot {
  const state = (window.history.state ?? {}) as HistoryState;
  return { route: parseHash(window.location.hash), from: state.aeFrom ?? null, kind };
}

let snapshot: RouterSnapshot | null = null;
const listeners = new Set<() => void>();

function emit(kind: RouterSnapshot["kind"]) {
  snapshot = read(kind);
  listeners.forEach((l) => l());
}

/**
 * A guard returns true to allow leaving the page, false to stay. A guard that
 * asks first (an in-app dialog) returns false and later calls `proceed` to
 * finish the navigation it held back.
 */
type Guard = (proceed: () => void) => boolean;
const guards = new Set<Guard>();

function guardsAllow(proceed: () => void): boolean {
  for (const g of guards) if (!g(proceed)) return false;
  return true;
}

let lastHash = typeof window !== "undefined" ? window.location.hash : "";

// popstate covers back/forward and a hand-edited hash; pushState never fires it.
function onPop() {
  if (window.location.hash === lastHash) return;
  const wanted = window.location.hash.replace(/^#/, "") || DEFAULT_PATH;
  if (!guardsAllow(() => navigate(wanted))) {
    // Undo the browser's move: put the page the user is still on back on top.
    window.history.pushState(window.history.state, "", lastHash || "#" + DEFAULT_PATH);
    return;
  }
  // A bare "#/" (or an emptied hash) is the app root: the dashboard, as on
  // first load, not "Page not found".
  if (wanted === "/" || (wanted === DEFAULT_PATH && !window.location.hash.startsWith("#/"))) window.history.replaceState(window.history.state, "", "#" + DEFAULT_PATH);
  lastHash = window.location.hash;
  emit("pop");
}

function subscribe(listener: () => void) {
  listeners.add(listener);
  if (listeners.size === 1) {
    window.addEventListener("popstate", onPop);
  }
  return () => {
    listeners.delete(listener);
    if (listeners.size === 0) {
      window.removeEventListener("popstate", onPop);
    }
  };
}

function getSnapshot(): RouterSnapshot {
  if (!snapshot) snapshot = read("initial");
  return snapshot;
}

export interface NavigateOptions {
  replace?: boolean;
}

/** Navigates to an in-app path ("/bots/dca?preset=x"). Returns false if a guard kept the user. */
export function navigate(to: string, opts: NavigateOptions = {}): boolean {
  const target = "#" + (to.startsWith("/") ? to : "/" + to);
  if (target === window.location.hash && !opts.replace) return true;
  if (!guardsAllow(() => navigate(to, opts))) return false;
  const current = parseHash(window.location.hash);
  const state: HistoryState = { aeFrom: current.path };
  if (opts.replace) window.history.replaceState(state, "", target);
  else window.history.pushState(state, "", target);
  lastHash = target;
  emit("push");
  return true;
}

export function back() {
  window.history.back();
}

export function forward() {
  window.history.forward();
}

/**
 * Puts the user on a real route once the shell mounts: the deep link they
 * arrived with if it was dropped on the way, otherwise the dashboard for an
 * empty or bare "#/" hash.
 */
export function ensureInitialRoute() {
  const hash = window.location.hash;
  if (hash && hash !== "#" && hash !== "#/") {
    lastHash = hash;
    return;
  }
  const replay = INITIAL_HASH && INITIAL_HASH !== "#" && INITIAL_HASH !== "#/" ? INITIAL_HASH : null;
  window.history.replaceState({}, "", replay ?? "#" + DEFAULT_PATH);
  lastHash = window.location.hash;
  emit("initial");
}

export function useRouter(): RouterSnapshot {
  return useSyncExternalStore(subscribe, getSnapshot, getSnapshot);
}

export function useRoute(): ParsedRoute {
  return useRouter().route;
}

/**
 * Registers a leave-guard while `active` (an unsaved form). `allow` decides:
 * true lets the user leave; false holds the navigation, and calling `proceed`
 * later (after an in-app "Discard changes?") completes it.
 */
export function useNavigationGuard(active: boolean, allow: Guard) {
  useEffect(() => {
    if (!active) return;
    guards.add(allow);
    return () => {
      guards.delete(allow);
    };
  }, [active, allow]);
}

/** Replaces one query parameter of the current route (tabs, filters). */
export function useQueryParam(name: string): [string | null, (value: string | null) => void] {
  const { route } = useRouter();
  const value = route.query.get(name);
  const set = useCallback(
    (next: string | null) => {
      const q = new URLSearchParams(route.query);
      if (next === null || next === "") q.delete(name);
      else q.set(name, next);
      const s = q.toString();
      navigate(s ? `${route.path}?${s}` : route.path, { replace: true });
    },
    [name, route],
  );
  return [value, set];
}

/** Per-route scroll memory for the center panel (in memory only). */
const scrollMemory = new Map<string, number>();

export function rememberScroll(key: string, top: number) {
  scrollMemory.set(key, top);
}

export function recallScroll(key: string): number | null {
  return scrollMemory.get(key) ?? null;
}

interface LinkProps extends Omit<AnchorHTMLAttributes<HTMLAnchorElement>, "href"> {
  /** In-app path, e.g. "/bots/dca". */
  to: string;
  replace?: boolean;
}

/** An anchor that navigates in-app; a modified click keeps default behaviour off. */
export function Link({ to, replace, onClick, children, ...rest }: LinkProps) {
  const href = "#" + (to.startsWith("/") ? to : "/" + to);
  const handle = (e: MouseEvent<HTMLAnchorElement>) => {
    onClick?.(e);
    if (e.defaultPrevented) return;
    e.preventDefault();
    navigate(to, { replace });
  };
  return (
    <a href={href} onClick={handle} {...rest}>
      {children}
    </a>
  );
}
