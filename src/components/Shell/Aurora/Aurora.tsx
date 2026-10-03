import "./Aurora.css";

/**
 * Decorative full-bleed backdrop for the app shell: a soft accent glow over a
 * faint perspective grid. Purely presentational (pointer-events: none, aria
 * hidden) and entirely token-driven — in the contrast theme --glow and --grid
 * resolve to transparent, so the layer vanishes for maximum legibility, and it
 * stays still under prefers-reduced-motion.
 */
export function Aurora() {
  return (
    <div className="ae-aurora" aria-hidden="true">
      <div className="ae-aurora__grid" />
      <div className="ae-aurora__glow" />
    </div>
  );
}
