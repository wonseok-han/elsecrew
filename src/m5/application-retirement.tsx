import { useEffect, useRef, useState } from "react";
import { useI18n } from "../i18n.tsx";
import { validateProjectCatalogResult, type ApplicationRetirementInspection } from "./contracts/project-catalog-v1.ts";

type Props = {projectId: string; applicationId: string; disabled: boolean; onRetired: () => void};
export function ApplicationRetirement(props: Props) {
  return <RetirementSession key={JSON.stringify([props.projectId, props.applicationId])} {...props} />;
}
function RetirementSession({projectId, applicationId, disabled, onRetired}: Props) {
  const { t } = useI18n();
  const [inspection, setInspection] = useState<ApplicationRetirementInspection | null>(null);
  const [enabled, setEnabled] = useState(false);
  const [agreed, setAgreed] = useState(false);
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<"" | "error" | "unknown">("");
  const epoch = useRef(0);
  const pending = useRef(false);
  useEffect(() => {
    epoch.current++; setInspection(null); setEnabled(false); setAgreed(false); setMessage("");
    return () => { epoch.current++; };
  }, [disabled]);
  async function run(retire = false) {
    if (disabled || pending.current || !window.m5Projects || (retire && (!enabled || !agreed || !inspection))) return;
    const generation = epoch.current;
    const request = retire ? {action:"retireApplication" as const,projectId,applicationId,fingerprint:inspection!.fingerprint}
      : {action:"inspectApplicationRetirement" as const,projectId,applicationId};
    pending.current = true; setBusy(true); setInspection(null); setEnabled(false); setAgreed(false); setMessage("");
    try {
      const result = validateProjectCatalogResult(await window.m5Projects.request(request), request);
      if (epoch.current !== generation) return;
      if (result.action === "inspectApplicationRetirement") { setInspection(result.inspection); setEnabled(result.enabled); }
      else if (result.action === "retireApplication") onRetired();
    } catch { if (epoch.current === generation) setMessage(retire ? "unknown" : "error"); }
    finally { pending.current = false; setBusy(false); }
  }
  return <section aria-label={t("m5.retire.title")} aria-busy={busy}>
    <button type="button" disabled={disabled || busy} onClick={() => void run()}>{t("m5.retire.title")}</button>
    {inspection && <>
      <p>{t("m5.retire.help")}</p>
      {!enabled && <p>{t("m5.retire.disabled")}</p>}
      {enabled && <>
        <label><input type="checkbox" checked={agreed} disabled={disabled || busy} onChange={event => setAgreed(event.target.checked)} />{t("m5.retire.consent")}</label>
        <button type="button" disabled={disabled || busy || !agreed} onClick={() => void run(true)}>{t("m5.retire.confirm")}</button>
      </>}
    </>}
    {message && <p role="status">{t(`m5.retire.${message}`)}</p>}
  </section>;
}
