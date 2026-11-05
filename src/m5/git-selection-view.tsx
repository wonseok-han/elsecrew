import { useEffect, useRef, useState } from "react";
import { useI18n } from "../i18n.tsx";
import { validateGitSelection, type GitSelection, type GitSelectionAction } from "./contracts/git-selection-v1.ts";

export function GitSelectionView({ disabled }: { disabled: boolean }) {
  const { t } = useI18n();
  const [selection, setSelection] = useState<GitSelection | null>(null);
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
    setBusy(true); setFailed(false);
    try {
      const value = validateGitSelection(await window.m5GitSelection.request(action));
      if (owner.current === generation) setSelection(value);
    } catch { if (owner.current === generation) setFailed(true); }
    finally {
      if (owner.current === generation) { pending.current = false; setBusy(false); }
    }
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
    </section>
  </details>;
}
