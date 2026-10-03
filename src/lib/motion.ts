/**
 * Motion presets — the single source of truth for animation feel.
 *
 * Grounded in Emil Kowalski's rules: custom easing curves (built-ins are too
 * weak), UI durations under 300ms, subtle springs (bounce 0.1–0.3) only where
 * an element should feel "alive". Durations mirror tokens.css so CSS and JS
 * motion stay in lockstep.
 */
import type { Transition } from "framer-motion";

/** Strong custom easings (match tokens.css). */
export const EASE = {
  out: [0.23, 1, 0.32, 1],
  inOut: [0.77, 0, 0.175, 1],
  drawer: [0.32, 0.72, 0, 1],
} as const;

export const DUR = {
  press: 0.14,
  fast: 0.16,
  pop: 0.2,
  panel: 0.26,
} as const;

/** Apple-style spring — easy to reason about (duration + bounce). */
export const SPRING = {
  /** The sliding-indicator feel (theme toggle pill). Alive but disciplined. */
  toggle: { type: "spring", duration: 0.42, bounce: 0.22 } as Transition,
  /** Softer, for panels/cards settling into place. */
  soft: { type: "spring", duration: 0.5, bounce: 0.12 } as Transition,
  /** Snappy, minimal bounce — for small state morphs. */
  snappy: { type: "spring", duration: 0.32, bounce: 0.1 } as Transition,
} as const;

/**
 * Standard entrance: never from scale(0) — start at 0.96 + opacity so it
 * enters like a real object, not from nothing.
 */
export const enter = {
  initial: { opacity: 0, scale: 0.96 },
  animate: { opacity: 1, scale: 1 },
  exit: { opacity: 0, scale: 0.98 },
  transition: { duration: DUR.pop, ease: EASE.out },
} as const;

/** True when the user asked the OS to reduce motion. */
export function prefersReducedMotion(): boolean {
  return (
    typeof window !== "undefined" &&
    window.matchMedia("(prefers-reduced-motion: reduce)").matches
  );
}

/**
 * Transition for `height: 0 → auto` collapsibles. `MotionConfig
 * reducedMotion="user"` disables transforms/layout but NOT height (it isn't a
 * transform), so this collapses the duration to 0 under reduced-motion to
 * honor the setting for these panels.
 */
export function collapseTransition() {
  return { duration: prefersReducedMotion() ? 0 : DUR.pop, ease: EASE.out };
}
