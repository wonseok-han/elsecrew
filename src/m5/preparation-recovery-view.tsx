import { useI18n } from "../i18n.tsx";
import { useEffect, useRef, useState } from "react";
import { validateProjectCatalogResult, type PreparationInspection, type ProjectCatalogRequest } from "./contracts/project-catalog-v1.ts";

export function PreparationRecovery({ projectId, disabled }: { projectId: string; disabled: boolean }) {
  const { t } = useI18n();
  const [inspection, setInspection] = useState<PreparationInspection | null>(null);
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<"" | "done" | "unknown" | "error">("");
  const epoch = useRef(0);
  const pending = useRef(false);
  useEffect(() => {
    epoch.current++; setInspection(null); setMessage("");
    return () => { epoch.current++; };
  }, [projectId, disabled]);
  const request = async (input: ProjectCatalogRequest) => {
    if (disabled || pending.current || !window.m5Projects) return;
    const generation = epoch.current;
    pending.current = true; setBusy(true); setMessage("");
    try {
      const result = validateProjectCatalogResult(await window.m5Projects.request(input), input);
      if (epoch.current !== generation) return;
      if (result.action === "inspectPreparation") setInspection(result.inspection);
      else if (result.action === "abandonPreparation") {
        setInspection(null);
        setMessage("done");
      }
    } catch {
      if (epoch.current === generation) setMessage(input.action === "abandonPreparation"
        ? "unknown"
        : "error");
    } finally { pending.current = false; setBusy(false); }
  };
  return <div aria-label={t("m5.prep.title")} aria-busy={busy}>
    <button type="button" disabled={disabled || busy} onClick={() => { setInspection(null); void request({ action: "inspectPreparation", projectId }); }}>{t("m5.prep.inspect")}</button>
    {inspection && <div role="group" aria-label={t("m5.prep.confirm")}>
      <p>{t("m5.prep.help")}</p>
      <code>{inspection.worktreeId}</code>
      <button type="button" disabled={disabled || busy} onClick={() => void request({ action: "abandonPreparation", inspection })}>{t("m5.prep.abandon")}</button>
      <button type="button" disabled={busy} onClick={() => setInspection(null)}>{t("m5.prep.back")}</button>
    </div>}
    {message && <p role="status">{t(`m5.prep.${message}`)}</p>}
  </div>;
}
