import { useI18n } from "../i18n.tsx";
import { useEffect, useRef, useState, type ReactNode } from "react";
import type { SupportedM5Bridge } from "./bridge-boundary.ts";
import type { M5BridgeV6 } from "./contracts/bridge-v6.ts";
import { summarizeWorkStatus, type WorkStatusSummary } from "./work-status.ts";

/** Read-only status: no automatic cancellation, lock release, or recovery mutation. */
export function WorkStatusNotice({ bridge, ready, refreshKey, feedback, errors, errorTitle }: {
  bridge: SupportedM5Bridge; ready: boolean; refreshKey: string;
  feedback?: ReactNode; errors?: ReactNode; errorTitle?: string;
}) {
  const { t } = useI18n();
  const [summary, setSummary] = useState<WorkStatusSummary | null>(null);
  const [failed, setFailed] = useState(false);
  const [busy, setBusy] = useState(false);
  const [refresh, setRefresh] = useState(0);
  const inFlight = useRef<Promise<unknown> | null>(null);
  useEffect(() => {
    let current = true;
    if (!ready || bridge.version < 6) { setSummary(null); setFailed(false); return; }
    setBusy(true);
    // Serialize behind an obsolete read; its result must never update this view.
    const previous = inFlight.current;
    const request = (async () => {
      if (previous) await previous.catch(() => {});
      if (!current) return;
      try {
        const status = await (bridge as M5BridgeV6).getSystemStatus();
        if (!current) return;
        setSummary(summarizeWorkStatus(status)); setFailed(false);
      } catch { if (current) setFailed(true); }
      finally { if (current) setBusy(false); }
    })();
    inFlight.current = request;
    return () => { current = false; };
  }, [bridge, ready, refreshKey, refresh]);
  const hasWorkStatus = ready && bridge.version >= 6 && (summary !== null || failed);
  if (!hasWorkStatus && !errorTitle) return <div className="m5-inline-feedback" role="status">{feedback}</div>;
  return <details className={`m5-status-strip${errorTitle || failed ? " m5-status-strip-attention" : ""}`}>
    <summary>
      <span className="m5-status-indicator" aria-hidden="true" />
      <span className="m5-status-label" role="status">{errorTitle || (failed ? t("m5.work.failed") : t("m5.work.active"))}</span>
      <span className="m5-status-details-label">{t("m5.feedback.details")}</span>
    </summary>
    <div className="m5-status-content">
      {errors}
      <div className="m5-inline-feedback">{feedback}</div>
      {hasWorkStatus && <section aria-label={t("m5.work.title")} aria-busy={busy}>
        <strong>{failed ? t("m5.work.failed") : t("m5.work.active")}</strong>
        <p>{failed ? t("m5.work.error") : t("m5.work.help")}</p>
        <p>{t("m5.work.safety")}</p>
        {summary && <details><summary>{t("m5.work.details", { count: summary.total })}{failed ? t("m5.work.stale") : ""}</summary>
          <ul>{summary.ids.map((id, index) => <li key={index}><code>{id}</code></li>)}</ul>
          {summary.omitted > 0 && <p>{t("m5.work.omitted", { count: summary.omitted })}</p>}
        </details>}
        <button type="button" className="m5-secondary" disabled={busy} onClick={() => setRefresh(value => value + 1)}>{busy ? t("m5.work.busy") : t("m5.work.refresh")}</button>
      </section>}
    </div>
  </details>;
}
