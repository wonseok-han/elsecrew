import { useEffect, useRef, useState } from "react";
import type { AgentRunCancelRequestV1 } from "./contracts/agent-adapter-v1.ts";
import { validateApprovalResult, visibleApprovalText, type ApprovalInspection, type ApprovalSummary } from "./contracts/approval-v1.ts";
import { approvalHeadline, withdrawnRequests } from "./approval-display.ts";

import { useI18n } from "../i18n.tsx";

/** Ephemeral requests only. No approval state is restored from local storage. */
export function ApprovalCard({ identity, onCancel, onPendingChange }: { identity: AgentRunCancelRequestV1; onCancel(): void;
  /** How many requests wait for the person (the task shows "needs you"). */
  onPendingChange?: (count: number) => void }) {
  const { t } = useI18n();
  const [pending, setPending] = useState<ApprovalSummary[]>([]);
  const [error, setError] = useState(false);
  const [sending, setSending] = useState(false);
  const [queued, setQueued] = useState(false);
  const [inspection, setInspection] = useState<ApprovalInspection | null>(null);
  /** Requests the agent took back before anyone answered: said briefly, then folded. */
  const [withdrawn, setWithdrawn] = useState<{ approvalId: string; kind: ApprovalSummary["kind"]; summary: string | null }[]>([]);
  const decided = useRef(new Set<string>());
  /** Reads tried per request: a failed read is tried again a few times, then said. */
  const autoInspected = useRef(new Map<string, number>());
  const [unreadable, setUnreadable] = useState<ReadonlySet<string>>(() => new Set());
  const lastPending = useRef<ApprovalSummary[]>([]);
  const lastInspection = useRef<ApprovalInspection | null>(null);
  lastInspection.current = inspection;
  const live = useRef(false);
  const busy = useRef(false);
  const epoch = useRef(0);
  const key = JSON.stringify(identity);
  useEffect(() => {
    live.current = true;
    epoch.current++;
    busy.current = false;
    setPending([]); setError(false); setSending(false); setQueued(false); setInspection(null); setWithdrawn([]);
    decided.current = new Set(); autoInspected.current = new Map(); lastPending.current = []; setUnreadable(new Set());
    let disposed = false;
    let timer: ReturnType<typeof setTimeout>;
    const bridge = window.m5Approval;
    const request = { action: "list" as const, identity };
    async function poll() {
      if (disposed) return;
      if (!busy.current && bridge) {
        const revision = epoch.current;
        try {
          const result = validateApprovalResult(await bridge.request(request), request);
          if (!disposed && revision === epoch.current) { setPending(result.pending); setError(false); setInspection(current => current && result.pending.some(item=>item.approvalId===current.approvalId && item.kind===current.kind) ? current : null); }
        } catch {
          if (!disposed && revision === epoch.current) { setPending([]); setError(true); setInspection(null); }
        }
      }
      if (!disposed) timer = setTimeout(() => void poll(), 750);
    }
    if (bridge) void poll();
    return () => { disposed = true; live.current = false; epoch.current++; clearTimeout(timer); };
    // The complete immutable identity is the lifecycle key, not object allocation.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key]);

  async function decide(approvalId: string, action: "decline" | "acceptOnce") {
    if (busy.current || !window.m5Approval) return;
    if (action === "acceptOnce" && inspection?.approvalId !== approvalId) return;
    busy.current = true;
    decided.current.add(approvalId);
    const revision = ++epoch.current;
    setSending(true);
    setInspection(null);
    const request = { action, identity, approvalId };
    try {
      const result = validateApprovalResult(await window.m5Approval.request(request), request);
      if (live.current && revision === epoch.current) { setPending(result.pending); setQueued(true); setError(false); }
    } catch {
      if (live.current && revision === epoch.current) { setPending([]); setError(true); }
    } finally {
      if (live.current && revision === epoch.current) { busy.current = false; setSending(false); }
    }
  }

  async function inspect(approvalId: string) {
    if (busy.current || !window.m5Approval) return;
    if (inspection?.approvalId === approvalId) { setInspection(null); return; }
    busy.current = true;
    const revision = ++epoch.current;
    setSending(true); setInspection(null);
    const request = {action:"inspect" as const, identity, approvalId};
    try {
      const result = validateApprovalResult(await window.m5Approval.request(request), request);
      if (live.current && revision === epoch.current) { setPending(result.pending); setInspection(result.inspection ?? null); setError(false); }
    } catch {
      if (live.current && revision === epoch.current) { setPending([]); setInspection(null); setError(true); }
    } finally {
      if (live.current && revision === epoch.current) { busy.current = false; setSending(false); }
    }
  }

  // The card's first line says what is asked: read each request as it comes.
  useEffect(() => {
    const first = pending[0];
    if (!first || sending || inspection) return;
    const tries = autoInspected.current.get(first.approvalId) ?? 0;
    if (tries >= 3) { setUnreadable(current => current.has(first.approvalId) ? current : new Set(current).add(first.approvalId)); return; }
    // After a failed read the list comes back on the next poll; try again then.
    if (error) return;
    autoInspected.current.set(first.approvalId, tries + 1);
    void inspect(first.approvalId);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [pending, error, sending, inspection]);
  useEffect(() => {
    const before = lastPending.current;
    lastPending.current = pending;
    if (error) return;
    const gone = withdrawnRequests(before, pending, decided.current);
    if (!gone.length) return;
    const shown = lastInspection.current;
    setWithdrawn(list => [...list, ...gone.map(item => ({ approvalId: item.approvalId, kind: item.kind, summary: shown?.approvalId === item.approvalId ? approvalHeadline(shown).target : null }))]);
    const ids = new Set(gone.map(item => item.approvalId));
    setTimeout(() => { if (live.current) setWithdrawn(list => list.filter(item => !ids.has(item.approvalId))); }, 8000);
  }, [pending, error]);
  useEffect(() => { onPendingChange?.(pending.length); }, [pending.length, onPendingChange]);
  useEffect(() => () => onPendingChange?.(0), [onPendingChange]);

  if (!pending.length && !error && !withdrawn.length) return null;
  return <section className="m5-approval-card" aria-label={t("m5.approval.title")}>
    <strong>{t("m5.approval.title")}</strong>
    {pending.map(request => {
      const shown = inspection?.approvalId === request.approvalId ? inspection : null;
      const headline = shown ? approvalHeadline(shown) : null;
      return <div key={request.approvalId} className="m5-approval-request">
        <p className="m5-approval-line"><span className="m5-approval-kind">{t(`m5.approval.kind.${request.kind}`)}</span>
          {headline ? <code className="m5-approval-target" dir="auto">{headline.target}</code> : <span className="m5-approval-reading">{t(unreadable.has(request.approvalId) ? "m5.approval.unreadable" : "m5.approval.reading")}</span>}</p>
        {headline?.reason && <p className="m5-approval-why" dir="auto">{t("m5.approval.why", { reason: headline.reason })}</p>}
        {shown && <p className="m5-approval-accept-note">{t("m5.approval.acceptWarning")}</p>}
        <div className="m5-approval-actions">
          {shown && <button type="button" disabled={sending || error} onClick={() => void decide(request.approvalId, "acceptOnce")}>{t("m5.approval.acceptOnce")}</button>}
          <button type="button" className="m5-secondary" disabled={sending || error} onClick={() => void decide(request.approvalId, "decline")} aria-label={t("m5.approval.declineLabel", { kind: t(`m5.approval.kind.${request.kind}`) })}>{t("m5.approval.decline")}</button>
          {!shown && <button type="button" className="m5-secondary" disabled={sending || error} aria-expanded={false} onClick={() => void inspect(request.approvalId)}>{t("m5.approval.view")}</button>}
        </div>
        {shown && <details className="m5-approval-details" aria-label={t("m5.approval.details")}><summary>{t("m5.feedback.details")}</summary><p>{t("m5.approval.warning")}</p><dl>{([[t("m5.approval.command"),shown.command],[t("m5.approval.cwd"),shown.cwd],[t("m5.approval.reason"),shown.reason],[t("m5.approval.toolInput"),shown.toolInput ?? null],[t("m5.approval.grantRoot"),shown.grantRoot],[t("m5.approval.host"),shown.host],[t("m5.approval.protocol"),shown.protocol]] as const).map(([label,value])=>value !== null && <div key={label}><dt>{label}</dt><dd><pre style={{whiteSpace:"pre-wrap",overflowWrap:"anywhere"}}>{visibleApprovalText(value)}</pre></dd></div>)}</dl></details>}
      </div>;
    })}
    {pending.length > 0 && <details className="m5-approval-help"><summary>{t("m5.approval.helpTitle")}</summary><p>{t("m5.approval.help")}</p></details>}
    {withdrawn.map(item => <p key={item.approvalId} role="status" className="m5-approval-withdrawn">{t("m5.approval.withdrawn")}{item.summary ? <> · <code dir="auto">{item.summary}</code></> : <> · {t(`m5.approval.kind.${item.kind}`)}</>}</p>)}
    {queued && <p role="status">{t("m5.approval.queued")}</p>}
    {error && <p role="alert">{t("m5.approval.error")}</p>}
    {(pending.length > 0 || error) && <button type="button" className="m5-secondary" onClick={onCancel}>{t("m5.approval.cancel")}</button>}
  </section>;
}

