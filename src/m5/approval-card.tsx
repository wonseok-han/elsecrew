import { useEffect, useRef, useState } from "react";
import type { AgentRunCancelRequestV1 } from "./contracts/agent-adapter-v1.ts";
import { validateApprovalResult, visibleApprovalText, type ApprovalInspection, type ApprovalSummary } from "./contracts/approval-v1.ts";

import { useI18n } from "../i18n.tsx";

/** Ephemeral requests only. No approval state is restored from local storage. */
export function ApprovalCard({ identity, onCancel }: { identity: AgentRunCancelRequestV1; onCancel(): void }) {
  const { t } = useI18n();
  const [pending, setPending] = useState<ApprovalSummary[]>([]);
  const [error, setError] = useState(false);
  const [sending, setSending] = useState(false);
  const [queued, setQueued] = useState(false);
  const [inspection, setInspection] = useState<ApprovalInspection | null>(null);
  const live = useRef(false);
  const busy = useRef(false);
  const epoch = useRef(0);
  const key = JSON.stringify(identity);
  useEffect(() => {
    live.current = true;
    epoch.current++;
    busy.current = false;
    setPending([]); setError(false); setSending(false); setQueued(false); setInspection(null);
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

  async function decline(approvalId: string) {
    if (busy.current || !window.m5Approval) return;
    busy.current = true;
    const revision = ++epoch.current;
    setSending(true);
    setInspection(null);
    const request = { action: "decline" as const, identity, approvalId };
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

  if (!pending.length && !error && !queued) return null;
  return <section className="m5-error" aria-label={t("m5.approval.title")}>
    <strong>{t("m5.approval.title")}</strong>
    {pending.length > 0 && <p>{t("m5.approval.help")}</p>}
    {pending.map((request, index) => <div key={request.approvalId}><span>{t("m5.approval.pending", { index: index + 1, kind: t(`m5.approval.kind.${request.kind}`) })} </span><button type="button" disabled={sending || error} aria-expanded={inspection?.approvalId === request.approvalId} onClick={() => void inspect(request.approvalId)}>{t(inspection?.approvalId === request.approvalId ? "m5.approval.close" : "m5.approval.view")}</button><button type="button" disabled={sending || error} onClick={() => void decline(request.approvalId)} aria-label={t("m5.approval.declineLabel", { kind: t(`m5.approval.kind.${request.kind}`) })}>{t("m5.approval.decline")}</button>
      {inspection?.approvalId === request.approvalId && <div aria-label={t("m5.approval.details")}><p>{t("m5.approval.warning")}</p><dl>{([[t("m5.approval.command"),inspection.command],[t("m5.approval.cwd"),inspection.cwd],[t("m5.approval.reason"),inspection.reason],[t("m5.approval.toolInput"),inspection.toolInput ?? null],[t("m5.approval.grantRoot"),inspection.grantRoot],[t("m5.approval.host"),inspection.host],[t("m5.approval.protocol"),inspection.protocol]] as const).map(([label,value])=>value !== null && <div key={label}><dt>{label}</dt><dd><pre style={{whiteSpace:"pre-wrap",overflowWrap:"anywhere"}}>{visibleApprovalText(value)}</pre></dd></div>)}</dl></div>}
    </div>)}
    {queued && <p role="status">{t("m5.approval.queued")}</p>}
    {error && <p role="alert">{t("m5.approval.error")}</p>}
    <button type="button" className="m5-secondary" onClick={onCancel}>{t("m5.approval.cancel")}</button>
  </section>;
}
