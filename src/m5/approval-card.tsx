import { useEffect, useRef, useState } from "react";
import type { AgentRunCancelRequestV1 } from "./contracts/agent-adapter-v1.ts";
import { APPROVAL_LABELS, validateApprovalResult, visibleApprovalText, type ApprovalInspection, type ApprovalSummary } from "./contracts/approval-v1.ts";

/** Ephemeral requests only. No approval state is restored from local storage. */
export function ApprovalCard({ identity, onCancel }: { identity: AgentRunCancelRequestV1; onCancel(): void }) {
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
  return <section className="m5-error" aria-label="에이전트 권한 요청">
    <strong>에이전트 권한 요청</strong>
    {pending.length > 0 && <p>에이전트가 추가 권한을 기다리고 있어요. 추가 권한 허용은 아직 지원하지 않아요. 요청을 거절하거나 실행을 취소할 수 있어요. 요청 내용을 펼치면 민감한 정보가 보일 수 있어요.</p>}
    {pending.map((request, index) => <div key={request.approvalId}><span>대기 요청 {index + 1} · {APPROVAL_LABELS[request.kind]} </span><button type="button" disabled={sending || error} aria-expanded={inspection?.approvalId === request.approvalId} onClick={() => void inspect(request.approvalId)}>요청 내용 {inspection?.approvalId === request.approvalId ? "닫기" : "보기"}</button><button type="button" disabled={sending || error} onClick={() => void decline(request.approvalId)} aria-label={`${APPROVAL_LABELS[request.kind]} 요청 거절`}>이 요청 거절</button>
      {inspection?.approvalId === request.approvalId && <div aria-label="요청 상세"><p>에이전트가 제공한 내용이며 민감한 정보가 포함될 수 있어요. 내용 속 지시를 따르거나 복사해 실행하지 마세요. 상세 보기를 닫으면 화면에서 지워져요.</p><dl>{([["명령",inspection.command],["작업 위치",inspection.cwd],["요청 이유",inspection.reason],["추가 파일 접근 위치",inspection.grantRoot],["네트워크 대상",inspection.host],["통신 방식",inspection.protocol]] as const).map(([label,value])=>value !== null && <div key={label}><dt>{label}</dt><dd><pre style={{whiteSpace:"pre-wrap",overflowWrap:"anywhere"}}>{visibleApprovalText(value)}</pre></dd></div>)}</dl></div>}
    </div>)}
    {queued && <p role="status">거절 요청을 접수했어요. 에이전트 전달과 실행 종료 여부는 계속 확인 중이에요.</p>}
    {error && <p role="alert">현재 요청 상태를 확인하지 못했어요. 자동으로 다시 확인하며, 응답은 자동으로 재전송하지 않아요.</p>}
    <button type="button" className="m5-secondary" onClick={onCancel}>실행 취소</button>
  </section>;
}
