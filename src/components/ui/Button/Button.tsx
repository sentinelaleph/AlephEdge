import { forwardRef, type ButtonHTMLAttributes, type ReactNode } from "react";
import { Tooltip, type TooltipAlign, type TooltipPlacement } from "@/components/ui/Tooltip/Tooltip";
import "./Button.css";

export type ButtonVariant = "primary" | "secondary" | "ghost" | "danger";
export type ButtonSize = "xs" | "sm" | "md";

interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: ButtonVariant;
  size?: ButtonSize;
  /**
   * Why the button is disabled: shown as a tooltip while `disabled` (hover
   * and keyboard focus both reach it). Ignored when enabled.
   */
  disabledReason?: ReactNode;
  tooltipPlacement?: TooltipPlacement;
  tooltipAlign?: TooltipAlign;
}

/**
 * The base pressable. Feedback is CSS-driven: transform: scale(0.97) on
 * :active with a fast ease-out transition, so every press feels heard
 * (emil-design-eng: buttons must feel responsive). No JS animation — presses
 * happen constantly and must be instant.
 *
 * Hierarchy (one rule for every view):
 *   primary    the ONE main action of the view (header: New bot, Run backtest,
 *              Save). Never in a table row.
 *   secondary  every other action in the header or a panel (Export CSV, Lock).
 *   ghost      quiet actions. Table row actions use size="xs" secondary
 *              (Start / Stop / Close), destructive ones included: a column
 *              of red row buttons is loud, and the click only opens a dialog.
 *   danger     the confirm button of a destructive ConfirmDialog (`danger`
 *              prop) or an armed inline confirmation (KeyList Remove).
 * Sizes: md 36px (forms, dialogs), sm 30px (header, panels), xs 26px (table rows).
 * Disabled keeps its label legible (no washed-out opacity); the reason goes
 * in `disabledReason`:
 *   <Button size="sm" disabled={!!blockKey} disabledReason={blockKey && t(blockKey)}>Start</Button>
 */
export const Button = forwardRef<HTMLButtonElement, ButtonProps>(
  (
    { variant = "primary", size = "md", className, type = "button", disabledReason, tooltipPlacement = "top", tooltipAlign = "center", ...rest },
    ref,
  ) => {
    const button = <button ref={ref} type={type} className={buttonClass(variant, size, className)} {...rest} />;
    if (!rest.disabled || !disabledReason) return button;
    return (
      <Tooltip content={disabledReason} placement={tooltipPlacement} align={tooltipAlign}>
        {button}
      </Tooltip>
    );
  },
);
Button.displayName = "Button";

/**
 * The same look for a router <Link> or <a> that acts as a button:
 *   <Link to="/bots/new" className={buttonClass("primary", "sm")}>New bot</Link>
 */
export function buttonClass(variant: ButtonVariant = "primary", size: ButtonSize = "md", extra?: string): string {
  return `ae-btn ae-btn--${variant} ae-btn--${size}${extra ? ` ${extra}` : ""}`;
}

interface IconButtonProps extends Omit<ButtonHTMLAttributes<HTMLButtonElement>, "children"> {
  /** Accessible name AND tooltip text. Required: the button has no visible text. */
  label: string;
  icon: ReactNode;
  size?: "xs" | "sm" | "md";
  /** Toggle state: sets aria-pressed and the pressed look. */
  pressed?: boolean;
  /** Pressed look only, when the state is already announced (aria-expanded on a panel toggle). */
  active?: boolean;
  tooltipPlacement?: TooltipPlacement;
  tooltipAlign?: TooltipAlign;
}

/**
 * An icon-only button with its label as aria-label and tooltip. Use for
 * header toggles and dense row actions where a word does not fit.
 *   <IconButton label={t("nav.summaryPanel")} icon={<Icon name="panelRight" />} active={open} aria-expanded={open} onClick={toggle} />
 */
export const IconButton = forwardRef<HTMLButtonElement, IconButtonProps>(
  ({ label, icon, size = "sm", pressed, active, tooltipPlacement = "bottom", tooltipAlign = "center", className, type = "button", ...rest }, ref) => (
    <Tooltip content={label} placement={tooltipPlacement} align={tooltipAlign}>
      <button
        ref={ref}
        type={type}
        aria-label={label}
        aria-pressed={pressed}
        data-pressed={pressed || active || undefined}
        className={`ae-iconbtn ae-iconbtn--${size}${className ? ` ${className}` : ""}`}
        {...rest}
      >
        {icon}
      </button>
    </Tooltip>
  ),
);
IconButton.displayName = "IconButton";
