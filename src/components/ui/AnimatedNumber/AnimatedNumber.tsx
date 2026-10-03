import { useEffect, useRef, useState } from "react";
import { animate } from "framer-motion";
import { EASE, prefersReducedMotion } from "@/lib/motion";

interface AnimatedNumberProps {
  value: number;
  /** Renders the (possibly mid-tween) number to a display string. */
  format: (n: number) => string;
}

/**
 * Spring-eased count-up when the value changes (PRD §4.3 "PnL sayaç tıkırtısı").
 * Pair with `font-variant-numeric: tabular-nums` on the parent so digits don't
 * jitter. Snaps instantly under reduced-motion.
 */
export function AnimatedNumber({ value, format }: AnimatedNumberProps) {
  const [display, setDisplay] = useState(value);
  const from = useRef(value);

  useEffect(() => {
    if (prefersReducedMotion()) {
      from.current = value;
      setDisplay(value);
      return;
    }
    // Track the live tween value in the ref (not the target) so an interrupted
    // tween resumes from what's on screen, not from the previous target.
    const controls = animate(from.current, value, {
      duration: 0.5,
      ease: EASE.out,
      onUpdate: (v) => {
        from.current = v;
        setDisplay(v);
      },
    });
    return () => controls.stop();
  }, [value]);

  return <>{format(display)}</>;
}
