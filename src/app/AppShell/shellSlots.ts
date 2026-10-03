import { createContext, useContext } from "react";

/**
 * Header slots a page fills through a portal: its heading (breadcrumb + h1)
 * and its primary actions. Null outside the shell (dev screenshots), where
 * PageShell renders both inline instead.
 */
export interface ShellSlots {
  heading: HTMLElement | null;
  actions: HTMLElement | null;
}

export const ShellSlotsContext = createContext<ShellSlots>({ heading: null, actions: null });

export function useShellSlots(): ShellSlots {
  return useContext(ShellSlotsContext);
}
