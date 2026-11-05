import { useEffect, useRef, useState } from "react";
import { useI18n } from "../i18n.tsx";
import { validateGitSelection, type GitSelection, type GitSelectionAction } from "./contracts/git-selection-v1.ts";

import { validateProjectCatalogResult, type GitTrustInspection, type ProjectCatalogRequest } from "./contracts/project-catalog-v1.ts";

export function GitSelectionView({ disabled }: { disabled: boolean }) {
  const { t } = useI18n();
  const [selection, setSelection] = useState<GitSelection | null>(null);
  const [trust, setTrust] = useState<GitTrustInspection | null>(null);
  const [accepted, setAccepted] = useState(false);
  const [trustFailed, setTrustFailed] = useState(false);
  const [busy, setBusy] = useState(false);
  const [failed, setFailed] = useState(false);
  const owner = useRef(0);
  const pending = useRef(false);
  useEffect(() => {
    const generation = ++owner.current;
    const bridge = window.m5GitSelection;
    if (bridge) {
      setBusy(true);
      pending.current = true;
      void bridge.request("inspect").then(value => {
        if (owner.current === generation) setSelection(validateGitSelection(value));
      }).catch(() => { if (owner.current === generation) setFailed(true); })
        .finally(() => {
          if (owner.current === generation) { pending.current = false; setBusy(false); }
        });
    }
    return () => { owner.current++; };
  }, []);
  async function request(action: GitSelectionAction) {
    if (disabled || pending.current || !window.m5GitSelection) return;
    const generation = owner.current;
    pending.current = true;
    setBusy(true); setFailed(false); setTrust(null); setAccepted(false); setTrustFailed(false);
    try {
      if (action === "forget") {
        if (!window.m5Projects) throw new Error("Git trust service unavailable");
        const input = { action: "revokeGitTrust" } as const;
        validateProjectCatalogResult(await window.m5Projects.request(input), input);
      }
      const value = validateGitSelection(await window.m5GitSelection.request(action));
      if (owner.current === generation) setSelection(value);
    } catch { if (owner.current === generation) setFailed(true); }
    finally {
      if (owner.current === generation) { pending.current = false; setBusy(false); }
    }
  }
  async function requestTrust(action: "inspectGitTrust" | "confirmGitTrust" | "revokeGitTrust") {
    if (disabled || pending.current || !window.m5Projects || selection?.state !== "selected") return;
    if (action === "confirmGitTrust" && (!trust || !accepted)) return;
    const input: ProjectCatalogRequest = action === "confirmGitTrust"
      ? { action, identity: trust!.identity, confirmed: true } : { action };
    const generation = owner.current;
    pending.current = true; setBusy(true); setTrustFailed(false); setAccepted(false);
    try {
      const result = validateProjectCatalogResult(await window.m5Projects.request(input), input);
      if (owner.current !== generation) return;
      if (result.action === "inspectGitTrust" || result.action === "confirmGitTrust") setTrust(result.inspection);
      else setTrust(null);
    } catch { if (owner.current === generation) { setTrust(null); setTrustFailed(true); } }
    finally { if (owner.current === generation) { pending.current = false; setBusy(false); } }
  }
  if ((!selection && !failed) || selection?.state === "unsupported") return null;
  return <details className="m5-notice">
    <summary>{t("m5.git.title")}</summary>
    <section aria-label={t("m5.git.title")} aria-busy={busy}>
      <p>{t("m5.git.intro")}</p>
      <p role="status">{t(selection?.state === "selected" ? "m5.git.selected" : selection?.state === "unavailable" ? "m5.git.unavailable" : "m5.git.unselected")}</p>
      {selection?.path && <p style={{ overflowWrap: "anywhere" }}>{selection.path}</p>}
      {failed && <p role="alert">{t("m5.git.failed")}</p>}
      <button type="button" className="m5-secondary" disabled={disabled || busy} onClick={() => void request("choose")}>{t("m5.git.choose")}</button>
      <button type="button" className="m5-secondary" disabled={disabled || busy} onClick={() => void request("inspect")}>{t("m5.git.refresh")}</button>
      {(selection?.state === "selected" || selection?.state === "unavailable") && <button type="button" className="m5-secondary" disabled={disabled || busy} onClick={() => void request("forget")}>{t("m5.git.forget")}</button>}
      {selection?.state === "selected" && <div>
        <button type="button" className="m5-secondary" disabled={disabled || busy || !window.m5Projects} onClick={() => void requestTrust("inspectGitTrust")}>{t("m5.git.inspectTrust")}</button>
        {trustFailed && <p role="alert">{t("m5.git.trustFailed")}</p>}
        {trust && <div>
          <p style={{ overflowWrap: "anywhere" }}>{trust.identity.installationPath}</p>
          <p>{t("m5.git.publisher")}: {trust.identity.commonName}{trust.identity.organization ? ` (${trust.identity.organization})` : ""}</p>
          <p>{t("m5.git.trustScope")}</p>
          <p role="status">{t(trust.trusted ? "m5.git.trusted" : "m5.git.needsTrust")}</p>
          {!trust.trusted && <label><input type="checkbox" checked={accepted} disabled={disabled || busy} onChange={event => setAccepted(event.target.checked)} />{t("m5.git.consent")}</label>}
          {!trust.trusted && <button type="button" disabled={disabled || busy || !accepted} onClick={() => void requestTrust("confirmGitTrust")}>{t("m5.git.confirmTrust")}</button>}
          {trust.trusted && <button type="button" className="m5-secondary" disabled={disabled || busy} onClick={() => void requestTrust("revokeGitTrust")}>{t("m5.git.revokeTrust")}</button>}
        </div>}
      </div>}
    </section>
  </details>;
}
