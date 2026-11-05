import { useEffect, useRef, useState } from "react";
import { useI18n } from "../i18n.tsx";
import type { ChatWorkspaceRefV1 } from "./contracts/workspace-addressing-v1.ts";
import { saveRecoveryDecision, validateSaveRecoveryResolveResult, validateSaveRecoveryResult, type SaveRecoveryResult } from "./contracts/save-recovery-v1.ts";

/** Inspected, explicitly confirmed recovery; the sidecar owns mutation admission. */
export function SaveRecoveryView({ address, disabled }: { address: ChatWorkspaceRefV1; disabled: boolean }) {
  // Remount the owner for every address/admission change so late results cannot
  // appear in a different chat or after admission is withdrawn.
  const key = JSON.stringify([address.chatId, address.projectId, address.worktreeId, address.workspaceIdentity, disabled]);
  return <SaveRecoveryOwner key={key} address={address} disabled={disabled} />;
}

function SaveRecoveryOwner({ address, disabled }: { address: ChatWorkspaceRefV1; disabled: boolean }) {
  const { t } = useI18n();
  const [supported, setSupported] = useState(false);
  const [busy, setBusy] = useState(false);
  const [failed, setFailed] = useState(false);
  const [confirmation, setConfirmation] = useState(false);
  const [resolution, setResolution] = useState<"committed" | "aborted" | "unknown" | null>(null);
  const [result, setResult] = useState<SaveRecoveryResult | null>(null);
  const live = useRef(false);
  const pending = useRef(false);
  useEffect(() => {
    let current = true;
    live.current = true;
    if (window.m5SaveRecovery && window.m5Device) {
      void window.m5Device.inspect().then(report => {
        if (current) setSupported(report.platform === "win32");
      }).catch(() => { if (current) setSupported(false); });
    }
    return () => { current = false; live.current = false; };
  }, []);

  async function inspect() {
    if (disabled || !supported || pending.current || !window.m5SaveRecovery) return;
    pending.current = true; setBusy(true); setFailed(false); setResult(null); setConfirmation(false); setResolution(null);
    try {
      const value = validateSaveRecoveryResult(await window.m5SaveRecovery.inspect(address), address);
      if (live.current) setResult(value);
    } catch {
      if (live.current) setFailed(true);
    } finally {
      pending.current = false;
      if (live.current) setBusy(false);
    }
  }
  async function resolve() {
    const recovery = result?.recovery;
    const decision = recovery ? saveRecoveryDecision(recovery) : null;
    if (disabled || pending.current || !confirmation || !recovery || !decision || !window.m5SaveRecovery) return;
    const request = { ...address, saveId: recovery.saveId, expectedLockRevision: recovery.lockRevision, decision, confirmed: true as const };
    pending.current = true; setBusy(true); setConfirmation(false); setResult(null); setResolution(null);
    try {
      const value = validateSaveRecoveryResolveResult(await window.m5SaveRecovery.resolve(request), request);
      if (live.current) setResolution(value.outcome);
    } catch {
      // The request may have reached the service. Reinspect before any retry.
      if (live.current) setResolution("unknown");
    } finally {
      pending.current = false;
      if (live.current) setBusy(false);
    }
  }
  if (!supported) return null;
  const recovery = result?.recovery;
  return <details className="m5-notice">
    <summary>{t("m5.saveRecovery.title")}</summary>
    <section aria-label={t("m5.saveRecovery.title")} aria-busy={busy}>
      <p>{t("m5.saveRecovery.intro")}</p>
      <button className="m5-secondary" type="button" disabled={disabled || busy} onClick={() => void inspect()}>
        {t(busy ? "m5.saveRecovery.checking" : "m5.saveRecovery.check")}
      </button>
      {resolution && <p role="status">{t(`m5.saveRecovery.${resolution}`)}</p>}
      {failed && <p role="alert">{t("m5.saveRecovery.error")}</p>}
      {result && !recovery && <p role="status">{t("m5.saveRecovery.empty")}</p>}
      {recovery && <div role="status">
        <p><strong>{recovery.path}</strong></p>
        <p>{t(`m5.saveRecovery.${recovery.observation}`)}</p>
        {saveRecoveryDecision(recovery) ? <>
          <p>{t(saveRecoveryDecision(recovery) === "complete" ? "m5.saveRecovery.completeHelp" : "m5.saveRecovery.abortHelp")}</p>
          {confirmation ? <div role="group" aria-label={t("m5.saveRecovery.confirm")}>
            <p>{t("m5.saveRecovery.confirmHelp")}</p>
            <button type="button" disabled={disabled || busy} onClick={() => void resolve()}>{t("m5.saveRecovery.confirm")}</button>
            <button type="button" disabled={busy} onClick={() => setConfirmation(false)}>{t("m5.saveRecovery.back")}</button>
          </div> : <button type="button" disabled={disabled || busy} onClick={() => setConfirmation(true)}>{t(saveRecoveryDecision(recovery) === "complete" ? "m5.saveRecovery.complete" : "m5.saveRecovery.abort")}</button>}
        </> : <p>{t("m5.saveRecovery.pending")}</p>}
        <details><summary>{t("m5.saveRecovery.evidence")}</summary>
          <dl><dt>{t("m5.saveRecovery.reference")}</dt><dd><code>{recovery.saveId}</code></dd>
            <dt>{t("m5.saveRecovery.phase")}</dt><dd><code>{recovery.phase}</code></dd>
            <dt>{t("m5.saveRecovery.revision")}</dt><dd>{recovery.lockRevision}</dd></dl>
        </details>
      </div>}
    </section>
  </details>;
}
