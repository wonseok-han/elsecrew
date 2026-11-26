import { useEffect, useRef, useState } from "react";
import type { AgentRunCancelRequestV1 } from "./contracts/agent-adapter-v1.ts";
import { validateApprovalResult, type ApprovalInspection, type ApprovalSummary } from "./contracts/approval-v1.ts";
import { approvalFields, approvalHeadline, assembleToolInput, canAcceptApproval, withdrawnRequests } from "./approval-display.ts";

import { useI18n } from "../i18n.tsx";

/** A request the agent took back before anyone answered it. */
export interface WithdrawnApproval {
  /** The task (conversation) the request belonged to: shown only there. */
  chatId: string;
  approvalId: string; kind: ApprovalSummary["kind"]; summary: string | null;
  /** The person answered, but the run ended before the answer was taken. */
  answerFailed?: boolean;
}

const FIELD_KEYS = {
  command: "m5.approval.command", cwd: "m5.approval.cwd", reason: "m5.approval.reason", toolInput: "m5.approval.toolInput",
  grantRoot: "m5.approval.grantRoot", host: "m5.approval.host", protocol: "m5.approval.protocol",
} as const;
const READ_TRIES = 3;

/**
 * Ephemeral requests only. No approval state is restored from local storage.
 *
 * Each waiting request is read on its own (one at a time): its first line
 * summarises what is asked and why. Allowing needs the person to open that
 * request's full content, shown exactly as the engine reported it.
 */
export function ApprovalCard({ identity, onCancel, onPendingChange, onWithdrawn }: { identity: AgentRunCancelRequestV1; onCancel(): void;
  /** How many requests wait for the person (the task shows "needs you"). */
  onPendingChange?: (count: number) => void;
  /** Requests the agent took back before anyone answered them, as seen here
   * or still waiting when this run's card goes away. The conversation keeps
   * the short notice, so it outlives this card (review #8). */
  onWithdrawn?: (items: WithdrawnApproval[]) => void }) {
  const { t } = useI18n();
  /** The last list the engine answered; an error never empties it. */
  const [pending, setPending] = useState<ApprovalSummary[]>([]);
  const [error, setError] = useState(false);
  const [sending, setSending] = useState(false);
  const [queued, setQueued] = useState(false);
  /** Full content per request, read from the engine for that exact request. */
  const [inspections, setInspections] = useState<ReadonlyMap<string, ApprovalInspection>>(() => new Map());
  /** Requests whose full content the person opened on this card. */
  const [opened, setOpened] = useState<ReadonlySet<string>>(() => new Set());
  /** Requests whose content could not be read after a few tries. */
  const [unreadable, setUnreadable] = useState<ReadonlySet<string>>(() => new Set());
  /** Large tool inputs read in full, every page joined and checked against
   * the engine's size and SHA-256 (PR #19 review B1). */
  const [fullInputs, setFullInputs] = useState<ReadonlyMap<string, string>>(() => new Map());
  /** Pages read so far of a large input being loaded, or a failed check. */
  const [fullProgress, setFullProgress] = useState<ReadonlyMap<string, number | "failed">>(() => new Map());
  /** Requests the engine confirmed as answered (allowed or declined). */
  const decided = useRef(new Set<string>());
  /** Requests being answered right now (not yet confirmed). */
  const deciding = useRef(new Set<string>());
  const tries = useRef(new Map<string, number>());
  const pendingRef = useRef<ApprovalSummary[]>([]);
  const inspectionsRef = useRef(inspections);
  inspectionsRef.current = inspections;
  const withdrawnCallback = useRef(onWithdrawn);
  withdrawnCallback.current = onWithdrawn;
  const live = useRef(false);
  const busy = useRef(false);
  const epoch = useRef(0);
  const key = JSON.stringify(identity);

  /** A new list from the engine: say what was taken back, forget what is gone. */
  const takeList = (next: ApprovalSummary[]) => {
    const gone = withdrawnRequests(pendingRef.current, next, new Set([...decided.current, ...deciding.current]));
    pendingRef.current = next;
    setPending(next);
    const still = new Set(next.map(item => item.approvalId));
    setInspections(current => [...current.keys()].every(id => still.has(id)) ? current : new Map([...current].filter(([id]) => still.has(id))));
    setOpened(current => [...current].every(id => still.has(id)) ? current : new Set([...current].filter(id => still.has(id))));
    setFullInputs(current => [...current.keys()].every(id => still.has(id)) ? current : new Map([...current].filter(([id]) => still.has(id))));
    setFullProgress(current => [...current.keys()].every(id => still.has(id)) ? current : new Map([...current].filter(([id]) => still.has(id))));
    if (!gone.length) return;
    withdrawnCallback.current?.(gone.map(item => ({ chatId: identity.chatId, approvalId: item.approvalId, kind: item.kind, summary: inspectionsRef.current.has(item.approvalId) ? approvalHeadline(inspectionsRef.current.get(item.approvalId)!).target : null })));
  };

  useEffect(() => {
    live.current = true;
    epoch.current++;
    busy.current = false;
    setPending([]); setError(false); setSending(false); setQueued(false); setInspections(new Map()); setOpened(new Set()); setUnreadable(new Set());
    setFullInputs(new Map()); setFullProgress(new Map());
    decided.current = new Set(); deciding.current = new Set(); tries.current = new Map(); pendingRef.current = [];
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
          if (!disposed && revision === epoch.current) { takeList(result.pending); setError(false); }
        } catch {
          // Keep the last list for comparison; nothing can be answered meanwhile.
          if (!disposed && revision === epoch.current) setError(true);
        }
      }
      if (!disposed) timer = setTimeout(() => void poll(), 750);
    }
    if (bridge) void poll();
    return () => {
      disposed = true; live.current = false; epoch.current++; clearTimeout(timer);
      // The run ended with requests still waiting: the agent no longer asks them.
      const left = pendingRef.current.filter(item => !decided.current.has(item.approvalId) && !deciding.current.has(item.approvalId));
      if (left.length) withdrawnCallback.current?.(left.map(item => ({ chatId: identity.chatId, approvalId: item.approvalId, kind: item.kind, summary: inspectionsRef.current.has(item.approvalId) ? approvalHeadline(inspectionsRef.current.get(item.approvalId)!).target : null })));
    };
    // The complete immutable identity is the lifecycle key, not object allocation.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key]);

  async function decide(approvalId: string, action: "decline" | "acceptOnce") {
    if (busy.current || !window.m5Approval) return;
    if (action === "acceptOnce" && !canAcceptApproval({ approvalId, inspectedId: inspections.get(approvalId)?.approvalId ?? null, openedIds: opened, sending, error,
      fullyRead: inspections.get(approvalId)?.toolInputPages === undefined || fullInputs.has(approvalId) })) return;
    busy.current = true;
    deciding.current.add(approvalId);
    const asked = pendingRef.current.find(item => item.approvalId === approvalId);
    const shown = inspections.get(approvalId);
    const revision = ++epoch.current;
    setSending(true);
    const request = { action, identity, approvalId };
    try {
      const result = validateApprovalResult(await window.m5Approval.request(request), request);
      // Only an answer the engine confirmed counts as answered.
      decided.current.add(approvalId);
      if (live.current && revision === epoch.current) { takeList(result.pending); setQueued(true); setError(false); }
    } catch {
      if (live.current && revision === epoch.current) setError(true);
      // The run ended while this answer was on its way, and it was not
      // taken: the conversation says so, apart from a request taken back.
      else if (!live.current && asked) withdrawnCallback.current?.([{ chatId: identity.chatId, approvalId, kind: asked.kind, summary: shown ? approvalHeadline(shown).target : null, answerFailed: true }]);
    } finally {
      deciding.current.delete(approvalId);
      if (live.current && revision === epoch.current) { busy.current = false; setSending(false); }
    }
  }

  async function inspect(approvalId: string) {
    if (busy.current || !window.m5Approval) return;
    busy.current = true;
    const revision = ++epoch.current;
    setSending(true);
    const request = { action: "inspect" as const, identity, approvalId };
    try {
      const result = validateApprovalResult(await window.m5Approval.request(request), request);
      if (live.current && revision === epoch.current) {
        takeList(result.pending);
        const inspection = result.inspection ?? null;
        if (inspection) setInspections(current => new Map(current).set(approvalId, inspection));
        setError(false);
      }
    } catch {
      if (live.current && revision === epoch.current) setError(true);
    } finally {
      if (live.current && revision === epoch.current) { busy.current = false; setSending(false); }
    }
  }

  /** Read every page of a large tool input, in order, then join and check it
   * against the engine's size and SHA-256. Only a checked whole is shown as
   * the full content and lets the person allow (PR #19 review B1). */
  async function loadFull(approvalId: string) {
    const first = inspections.get(approvalId);
    if (!first?.toolInputPages || fullInputs.has(approvalId) || fullProgress.has(approvalId) || busy.current || !window.m5Approval) return;
    busy.current = true;
    const revision = ++epoch.current;
    setSending(true);
    const pages = [first.toolInput ?? ""];
    setFullProgress(current => new Map(current).set(approvalId, 1));
    try {
      for (let page = 1; page < first.toolInputPages; page++) {
        const request = { action: "inspect" as const, identity, approvalId, page };
        const result = validateApprovalResult(await window.m5Approval.request(request), request);
        if (!live.current || revision !== epoch.current) return;
        const view = result.inspection;
        // Withdrawn meanwhile, or a page that names another input: stop.
        if (!view || view.toolInputSha256 !== first.toolInputSha256 || view.toolInputBytes !== first.toolInputBytes || view.toolInputPages !== first.toolInputPages) throw new Error("tool input changed");
        pages.push(view.toolInput ?? "");
        setFullProgress(current => new Map(current).set(approvalId, page + 1));
      }
      const text = await assembleToolInput(pages, first);
      if (live.current && revision === epoch.current) setFullInputs(current => new Map(current).set(approvalId, text));
    } catch {
      if (live.current && revision === epoch.current) setFullProgress(current => new Map(current).set(approvalId, "failed"));
    } finally {
      if (live.current && revision === epoch.current) { busy.current = false; setSending(false); }
    }
  }

  // Read each waiting request in turn, a few tries each, then say it could not.
  useEffect(() => {
    if (sending || error) return;
    for (const request of pending) {
      if (inspections.has(request.approvalId) || unreadable.has(request.approvalId)) continue;
      const count = tries.current.get(request.approvalId) ?? 0;
      if (count >= READ_TRIES) { setUnreadable(current => new Set(current).add(request.approvalId)); return; }
      tries.current.set(request.approvalId, count + 1);
      void inspect(request.approvalId);
      return;
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [pending, error, sending, inspections, unreadable]);
  useEffect(() => { onPendingChange?.(pending.length); }, [pending.length, onPendingChange]);
  useEffect(() => () => onPendingChange?.(0), [onPendingChange]);

  if (!pending.length && !error) return null;
  return <section className="m5-approval-card" aria-label={t("m5.approval.title")}>
    <strong>{t("m5.approval.title")}</strong>
    {pending.map(request => {
      const shown = inspections.get(request.approvalId) ?? null;
      const headline = shown ? approvalHeadline(shown) : null;
      const paged = shown?.toolInputPages !== undefined;
      const full = fullInputs.get(request.approvalId);
      const progress = fullProgress.get(request.approvalId);
      const accept = canAcceptApproval({ approvalId: request.approvalId, inspectedId: shown?.approvalId ?? null, openedIds: opened, sending, error, fullyRead: !paged || full !== undefined });
      return <div key={request.approvalId} className="m5-approval-request" data-approval-id={request.approvalId}>
        <p className="m5-approval-line"><span className="m5-approval-kind">{t(`m5.approval.kind.${request.kind}`)}</span>
          {headline ? <code className="m5-approval-target" dir="auto">{headline.target}</code>
            : <span className="m5-approval-reading">{t(unreadable.has(request.approvalId) ? "m5.approval.unreadable" : "m5.approval.reading")}</span>}</p>
        {headline?.reason && <p className="m5-approval-why" dir="auto">{t("m5.approval.why", { reason: headline.reason })}</p>}
        {shown && <details key={`full:${request.approvalId}`} className="m5-approval-details" aria-label={t("m5.approval.details")}
          onToggle={event => { if (event.currentTarget.open) { setOpened(current => current.has(request.approvalId) ? current : new Set(current).add(request.approvalId)); if (paged) void loadFull(request.approvalId); } }}>
          <summary>{t("m5.approval.fullContent")}</summary>
          <p>{t("m5.approval.warning")}</p>
          {paged && <p className="m5-approval-partial" role="status">{full !== undefined
            ? t("m5.approval.fullChecked", { size: formatApprovalBytes(shown.toolInputBytes ?? 0), sha256: shown.toolInputSha256 ?? "" })
            : progress === "failed" ? t("m5.approval.fullFailed")
            : t("m5.approval.fullLoading", { read: String(typeof progress === "number" ? progress : 0), pages: String(shown.toolInputPages) })}</p>}
          <dl>{approvalFields(paged ? (full !== undefined ? { ...shown, toolInput: full } : { ...shown, toolInput: undefined }) : shown)
            .map(item => <div key={item.field}><dt>{t(FIELD_KEYS[item.field])}</dt><dd><pre className="m5-approval-verbatim">{item.text}</pre></dd></div>)}</dl>
        </details>}
        {paged && full === undefined && <p className="m5-approval-partial">{t("m5.approval.large", { size: formatApprovalBytes(shown.toolInputBytes ?? 0) })}</p>}
        {shown && <p className="m5-approval-accept-note">{t(opened.has(request.approvalId) ? "m5.approval.acceptWarning" : "m5.approval.openFirst")}</p>}
        <div className="m5-approval-actions">
          {shown && <button type="button" disabled={!accept} onClick={() => void decide(request.approvalId, "acceptOnce")}>{t("m5.approval.acceptOnce")}</button>}
          <button type="button" className="m5-secondary" disabled={sending || error} onClick={() => void decide(request.approvalId, "decline")} aria-label={t("m5.approval.declineLabel", { kind: t(`m5.approval.kind.${request.kind}`) })}>{t("m5.approval.decline")}</button>
          {!shown && <button type="button" className="m5-secondary" disabled={sending || error} onClick={() => { tries.current.delete(request.approvalId); setUnreadable(current => { const next = new Set(current); next.delete(request.approvalId); return next; }); void inspect(request.approvalId); }}>{t("m5.approval.view")}</button>}
        </div>
      </div>;
    })}
    {pending.length > 0 && <details className="m5-approval-help"><summary>{t("m5.approval.helpTitle")}</summary><p>{t("m5.approval.help")}</p></details>}
    {queued && <p role="status">{t("m5.approval.queued")}</p>}
    {error && <p role="alert">{t("m5.approval.error")}</p>}
    {(pending.length > 0 || error) && <button type="button" className="m5-secondary" onClick={onCancel}>{t("m5.approval.cancel")}</button>}
  </section>;
}

/** "The agent took the request back", with what it was when known. */
export function ApprovalWithdrawnNote({ item }: { item: WithdrawnApproval }) {
  const { t } = useI18n();
  return <p role="status" className="m5-approval-withdrawn">{t(item.answerFailed ? "m5.approval.answerFailed" : "m5.approval.withdrawn")}{item.summary ? <> · <code dir="auto">{item.summary}</code></> : <> · {t(`m5.approval.kind.${item.kind}`)}</>}</p>;
}

/** A tool input's size for the partial-view note, in the units a person reads. */
export function formatApprovalBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(0)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}
