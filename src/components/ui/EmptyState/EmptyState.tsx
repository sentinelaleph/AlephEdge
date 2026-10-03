import type { ReactNode } from "react";
import "./EmptyState.css";

interface EmptyStateProps {
  title: string;
  /** One factual line under the title (no marketing copy). */
  detail?: string;
  actions?: ReactNode;
  tone?: "default" | "pending" | "error";
}

/** An honest empty, pending or unavailable state. Never sample numbers. */
export function EmptyState({ title, detail, actions, tone = "default" }: EmptyStateProps) {
  return (
    <div className="ae-empty" data-tone={tone} role={tone === "error" ? "alert" : undefined}>
      <p className="ae-empty__title">{title}</p>
      {detail ? <p className="ae-empty__detail">{detail}</p> : null}
      {actions ? <div className="ae-empty__actions">{actions}</div> : null}
    </div>
  );
}
