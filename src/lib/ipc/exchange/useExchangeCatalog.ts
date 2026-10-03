import { useCallback, useEffect, useRef, useState } from "react";
import { errorMessage } from "../bridge";
import { exchangeList, type ExchangeInfo } from "./exchange";

export interface ExchangeCatalog {
  /** Null while loading, or after a failed load. */
  exchanges: ExchangeInfo[] | null;
  /** The last load failure, shown instead of an empty picker. */
  error: string | null;
  retry: () => void;
}

/**
 * The exchange catalog from Rust (`exchange_list`). One shared request per
 * process (see `exchangeList`), so every picker and label can call this.
 * `fallbackError` is the translated message used when the rejection carries
 * no text of its own.
 */
export function useExchangeCatalog(fallbackError: string): ExchangeCatalog {
  const [exchanges, setExchanges] = useState<ExchangeInfo[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [attempt, setAttempt] = useState(0);
  const fallback = useRef(fallbackError);
  fallback.current = fallbackError;

  useEffect(() => {
    let alive = true;
    exchangeList()
      .then((list) => {
        if (!alive) return;
        setExchanges(list);
        setError(null);
      })
      .catch((e: unknown) => {
        if (alive) setError(errorMessage(e, fallback.current));
      });
    return () => {
      alive = false;
    };
  }, [attempt]);

  const retry = useCallback(() => setAttempt((n) => n + 1), []);
  return { exchanges, error, retry };
}
