import { useTranslation } from "react-i18next";
import { statusLabelKey, statusTone, type StatusKind } from "./statusTone";
import "./StatusChip.css";

interface StatusChipProps {
  status: StatusKind;
  /**
   * A more specific label for the same state ("Paused: daily stop",
   * "Stopped: budget"). Sentence case; the tone still comes from `status`.
   */
  label?: string;
  /** Why the state is what it is (shown on hover). */
  title?: string;
}

/**
 * Run / connection state with a fixed state -> tone map (statusTone.ts).
 *
 *   <StatusChip status="running" />
 *   <StatusChip status="notConfigured" />              -> amber "Not configured"
 *   <StatusChip status="paused" label={runStateText(t, v)} />
 *
 * Mode (Paper / LIVE) is not a status: the global mode chip lives in the
 * header only; rows show LIVE with LiveChip where real money is involved.
 */
export function StatusChip({ status, label, title }: StatusChipProps) {
  const { t } = useTranslation();
  return (
    <span className="ae-status" data-tone={statusTone(status)} data-status={status} title={title}>
      <span className="ae-status__dot" aria-hidden="true" />
      {label ?? t(statusLabelKey(status))}
    </span>
  );
}

export { statusTone, type StatusKind, type StatusTone } from "./statusTone";
