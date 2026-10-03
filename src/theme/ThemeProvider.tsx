import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
  type ReactNode,
} from "react";

/** User-facing theme preference. "system" follows the OS. */
export type ThemeMode = "light" | "dark" | "contrast" | "system";
/** The resolved theme actually applied to the DOM. */
export type ResolvedTheme = "light" | "dark" | "contrast";

const STORAGE_KEY = "aleph-edge-theme";

interface ThemeContextValue {
  /** The user's preference. */
  mode: ThemeMode;
  /** The concrete theme currently applied (system resolved). */
  resolved: ResolvedTheme;
  setMode: (mode: ThemeMode) => void;
  /** Convenience: cycle light → dark → contrast → system. */
  cycle: () => void;
}

const ThemeContext = createContext<ThemeContextValue | null>(null);

function systemPrefersDark(): boolean {
  return (
    typeof window !== "undefined" &&
    window.matchMedia("(prefers-color-scheme: dark)").matches
  );
}

function resolve(mode: ThemeMode): ResolvedTheme {
  if (mode === "system") return systemPrefersDark() ? "dark" : "light";
  return mode;
}

function readStoredMode(): ThemeMode {
  try {
    const v = localStorage.getItem(STORAGE_KEY);
    if (v === "light" || v === "dark" || v === "contrast" || v === "system") {
      return v;
    }
  } catch {
    /* ignore */
  }
  // Light is the product default; "system" stays selectable.
  return "light";
}

export function ThemeProvider({ children }: { children: ReactNode }) {
  const [mode, setModeState] = useState<ThemeMode>(readStoredMode);
  const [resolved, setResolved] = useState<ResolvedTheme>(() => resolve(mode));

  // Apply the resolved theme to the DOM and keep it in sync with `mode`.
  useEffect(() => {
    const next = resolve(mode);
    setResolved(next);
    document.documentElement.dataset.theme = next;
  }, [mode]);

  // When following the system, react to OS theme changes live.
  useEffect(() => {
    if (mode !== "system") return;
    const mq = window.matchMedia("(prefers-color-scheme: dark)");
    const onChange = () => {
      const next: ResolvedTheme = mq.matches ? "dark" : "light";
      setResolved(next);
      document.documentElement.dataset.theme = next;
    };
    mq.addEventListener("change", onChange);
    return () => mq.removeEventListener("change", onChange);
  }, [mode]);

  const setMode = useCallback((next: ThemeMode) => {
    setModeState(next);
    try {
      localStorage.setItem(STORAGE_KEY, next);
    } catch {
      /* ignore */
    }
  }, []);

  const cycle = useCallback(() => {
    // light → dark → contrast → system → light
    const next: ThemeMode =
      mode === "light"
        ? "dark"
        : mode === "dark"
          ? "contrast"
          : mode === "contrast"
            ? "system"
            : "light";
    setMode(next);
  }, [mode, setMode]);

  const value = useMemo<ThemeContextValue>(
    () => ({ mode, resolved, setMode, cycle }),
    [mode, resolved, setMode, cycle],
  );

  return (
    <ThemeContext.Provider value={value}>{children}</ThemeContext.Provider>
  );
}

export function useTheme(): ThemeContextValue {
  const ctx = useContext(ThemeContext);
  if (!ctx) throw new Error("useTheme must be used within <ThemeProvider>");
  return ctx;
}
