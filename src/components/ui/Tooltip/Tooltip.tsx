import {
  cloneElement,
  isValidElement,
  useCallback,
  useEffect,
  useId,
  useLayoutEffect,
  useRef,
  useState,
  type ReactElement,
  type ReactNode,
} from "react";
import { createPortal } from "react-dom";
import "./Tooltip.css";

export type TooltipPlacement = "top" | "bottom" | "right" | "left";
export type TooltipAlign = "start" | "center" | "end";

interface TooltipProps {
  /** Text or a small block (e.g. a FactList). Nothing renders when empty. */
  content: ReactNode;
  placement?: TooltipPlacement;
  align?: TooltipAlign;
  /**
   * One element. A focusable one (button, link) keeps its own focus; a
   * disabled button gets a focusable wrapper automatically (a disabled
   * control takes neither focus nor pointer events, so the wrapper opens the
   * tooltip). Static content (a chip) passes `focusable`.
   */
  children: ReactElement;
  /** The trigger is not focusable itself (a chip, a plain value): the wrapper takes focus. */
  focusable?: boolean;
}

const OPEN_DELAY = 300;
/** Another tooltip opened within this window opens instantly (no delay, no fade). */
const WARM_WINDOW = 400;
let lastClosedAt = 0;
let openCount = 0;

/**
 * Hover/focus tooltip, portalled to <body> so clipped containers (the status
 * bar, a scrolling table) never cut it. Opens after 300 ms; while one is open
 * or just closed, the next opens at once (emil-design-eng: skip the delay on
 * subsequent tooltips). Esc closes. Scales from the trigger side.
 *
 * Usage:
 *   <Tooltip content={t("nav.summaryPanel")}><button aria-label=…>…</button></Tooltip>
 *   <Tooltip content={<FactList rows={…} />} placement="bottom" align="end">…</Tooltip>
 *
 * The child gets aria-describedby; it keeps its own accessible name, so an
 * icon-only button still needs aria-label (IconButton does both).
 */
export function Tooltip({ content, placement = "top", align = "center", children, focusable }: TooltipProps) {
  const id = useId();
  const anchorRef = useRef<HTMLSpanElement>(null);
  const bubbleRef = useRef<HTMLDivElement>(null);
  const timer = useRef<number | undefined>(undefined);
  const [open, setOpen] = useState(false);
  const [instant, setInstant] = useState(false);
  const [pos, setPos] = useState<{ top: number; left: number } | null>(null);
  const empty = content === null || content === undefined || content === false || content === "";

  const show = useCallback(() => {
    window.clearTimeout(timer.current);
    const warm = openCount > 0 || Date.now() - lastClosedAt < WARM_WINDOW;
    if (warm) {
      setInstant(true);
      setOpen(true);
    } else {
      setInstant(false);
      timer.current = window.setTimeout(() => setOpen(true), OPEN_DELAY);
    }
  }, []);

  const hide = useCallback(() => {
    window.clearTimeout(timer.current);
    setOpen(false);
  }, []);

  useEffect(() => {
    if (!open) return;
    openCount += 1;
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && setOpen(false);
    document.addEventListener("keydown", onKey);
    return () => {
      openCount -= 1;
      lastClosedAt = Date.now();
      document.removeEventListener("keydown", onKey);
      setPos(null);
    };
  }, [open]);

  useEffect(() => () => window.clearTimeout(timer.current), []);

  // Place against the trigger, then keep it inside the viewport (8px margin).
  useLayoutEffect(() => {
    if (!open) return;
    const a = anchorRef.current?.firstElementChild ?? anchorRef.current;
    const b = bubbleRef.current;
    if (!a || !b) return;
    const r = a.getBoundingClientRect();
    const w = b.offsetWidth;
    const h = b.offsetHeight;
    const gap = 6;
    let top: number;
    let left: number;
    if (placement === "top" || placement === "bottom") {
      top = placement === "top" ? r.top - h - gap : r.bottom + gap;
      left = align === "start" ? r.left : align === "end" ? r.right - w : r.left + r.width / 2 - w / 2;
      // Flip when there is no room on the chosen side.
      if (placement === "top" && top < 8) top = r.bottom + gap;
      if (placement === "bottom" && top + h > window.innerHeight - 8) top = r.top - h - gap;
    } else {
      top = r.top + r.height / 2 - h / 2;
      left = placement === "right" ? r.right + gap : r.left - w - gap;
    }
    left = Math.max(8, Math.min(left, window.innerWidth - w - 8));
    top = Math.max(8, Math.min(top, window.innerHeight - h - 8));
    setPos({ top, left });
  }, [open, placement, align, content]);

  if (empty) return children;

  // A disabled control cannot be focused or hovered: the wrapper stands in.
  const childDisabled = isValidElement(children) && (children.props as { disabled?: unknown }).disabled === true;
  const proxy = focusable || childDisabled;

  const child =
    isValidElement(children) && !proxy
      ? cloneElement(children as ReactElement<{ "aria-describedby"?: string }>, {
          "aria-describedby": open ? id : undefined,
        })
      : children;

  return (
    <span
      ref={anchorRef}
      className="ae-tip-anchor"
      data-proxy={proxy || undefined}
      tabIndex={proxy ? 0 : undefined}
      aria-describedby={proxy && open ? id : undefined}
      onPointerEnter={show}
      onPointerLeave={hide}
      onFocus={show}
      onBlur={hide}
      onPointerDown={hide}
    >
      {child}
      {open
        ? createPortal(
            <div
              ref={bubbleRef}
              id={id}
              role="tooltip"
              className="ae-tip"
              data-side={placement}
              data-align={align}
              data-instant={instant || undefined}
              data-shown={pos ? true : undefined}
              style={pos ? { top: pos.top, left: pos.left } : { top: -9999, left: -9999 }}
            >
              {content}
            </div>,
            document.body,
          )
        : null}
    </span>
  );
}
