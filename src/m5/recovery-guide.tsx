// One way to say that something failed (ADE DESIGN 2.1 rule 5, F11): what
// happened → what you can do now (one main action, then the alternatives) →
// information for asking for help (folded, copyable). Used by the agent
// connection, and by errors and recovery elsewhere.
import { useState, type ReactNode } from "react";
import { useI18n } from "../i18n.tsx";

export interface GuideAction {
  label: string;
  onClick: () => void;
  disabled?: boolean;
  /** A screen reader name that also names the target (the visible label stays inside it). */
  ariaLabel?: string;
}

export function RecoveryGuide({ tone = "danger", title, summary, what, primary, alternatives = [], details, children, className = "", testId, live = true }: {
  tone?: "danger" | "attention";
  title: ReactNode;
  /** One reassuring line under the title (what did not happen). */
  summary?: ReactNode;
  what?: ReactNode;
  primary?: GuideAction | null;
  alternatives?: readonly GuideAction[];
  /** Plain text for asking for help: error keys, ids, versions. Never shown unfolded. */
  details?: string | null;
  children?: ReactNode;
  className?: string;
  testId?: string;
  /** Announce it (role=alert); off for a guide that is part of a page the person opened. */
  live?: boolean;
}) {
  const { t } = useI18n();
  const [copied, setCopied] = useState(false);
  const hasActions = Boolean(primary) || alternatives.length > 0;
  return <section className={`m5-recover m5-recover-${tone} ${className}`.trim()} role={live ? "alert" : undefined} data-testid={testId}>
    <header>
      <svg className="m5-recover-icon" viewBox="0 0 24 24" aria-hidden="true" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round"><path d="M12 4l9 16H3L12 4zM12 10v4.5M12 17.5v.01" /></svg>
      <div><h4>{title}</h4>{summary && <p>{summary}</p>}</div>
    </header>
    <div className="m5-recover-body">
      {what && <><p className="m5-recover-label">{t("m5.guide.what")}</p><p className="m5-recover-what">{what}</p></>}
      {children}
      {hasActions && <><p className="m5-recover-label">{t("m5.guide.now")}</p>
        <div className="m5-recover-actions">
          {primary && <button type="button" className="m5-btn-primary" aria-label={primary.ariaLabel} disabled={primary.disabled} onClick={primary.onClick}>{primary.label}</button>}
          {alternatives.map(action => <button key={action.label} type="button" className="m5-secondary" aria-label={action.ariaLabel} disabled={action.disabled} onClick={action.onClick}>{action.label}</button>)}
        </div></>}
      {details && <details className="m5-recover-details"><summary>{t("m5.guide.details")}</summary>
        <pre>{details}</pre>
        <button type="button" className="m5-btn-quiet" onClick={() => { void navigator.clipboard?.writeText(details).then(() => setCopied(true), () => setCopied(false)); }}>{t(copied ? "m5.guide.copied" : "m5.guide.copy")}</button>
      </details>}
    </div>
  </section>;
}
