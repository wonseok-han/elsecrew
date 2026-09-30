import { useI18n } from "../i18n.tsx";
import { useEffect, useRef, useState } from "react";
import type { ProjectEditorController } from "./editor-state.ts";
import { PreparationRecovery } from "./preparation-recovery-view.tsx";
import { validateProjectCatalogResult, type ProjectCatalogEntry } from "./contracts/project-catalog-v1.ts";

export function ProjectCatalog({ controller, disabled, currentId }: {
  controller: ProjectEditorController; disabled: boolean; currentId?: string;
}) {
  const { t } = useI18n();
  const [open, setOpen] = useState(false);
  const [entries, setEntries] = useState<ProjectCatalogEntry[]>([]);
  const [next, setNext] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState(false);
  const generation = useRef(0);
  const inFlight = useRef(false);
  useEffect(() => () => { generation.current++; }, []);
  useEffect(() => { generation.current++; setOpen(false); }, [disabled, currentId]);
  const service = window.m5Projects;
  if (!service) return null;
  const load = async (cursor: string | null) => {
    if (disabled || inFlight.current) return;
    const epoch = ++generation.current;
    inFlight.current = true; setBusy(true); setError(false); setOpen(true);
    try {
      const input = { action: "list" as const, cursor };
      const result = validateProjectCatalogResult(await service.request(input), input);
      if (epoch !== generation.current) return;
      if (result.action !== "list") throw new Error("unexpected project catalog response");
      setEntries(result.projects); setNext(result.nextCursor);
    } catch {
      if (epoch === generation.current) setError(true);
    } finally { inFlight.current = false; setBusy(false); }
  };
  const select = async (projectId: string) => {
    if (disabled || inFlight.current) return;
    inFlight.current = true; setBusy(true); generation.current++;
    try { await controller.reopenProject(projectId, input => service.request(input)); }
    finally { inFlight.current = false; setBusy(false); setOpen(false); }
  };
  return <section className="m5-project-catalog" aria-label={t("m5.catalog.title")}>
    <button type="button" className="m5-secondary" disabled={disabled || busy} onClick={() => open ? setOpen(false) : void load(null)} aria-expanded={open}>{t("m5.catalog.title")}</button>
    {open && <div className="m5-project-catalog-list" aria-busy={busy}>
      <p>{t("m5.catalog.order")}</p>
      {busy && <p role="status">{t("m5.catalog.busy")}</p>}
      {error ? <p role="alert">{t("m5.catalog.error")}</p> : !busy && entries.length === 0 ? <p>{t("m5.catalog.empty")}</p> :
        <ul>{entries.map(entry => <li key={entry.projectId}><button type="button" disabled={disabled || busy || entry.projectId === currentId} onClick={() => void select(entry.projectId)}>
          <strong>{entry.canonicalPath.split(/[\\/]/).filter(Boolean).pop() ?? entry.projectId}{entry.projectId === currentId ? t("m5.catalog.current") : ""}</strong>
          <span dir="auto">{entry.canonicalPath}</span>
        </button><PreparationRecovery projectId={entry.projectId} disabled={disabled || busy} /></li>)}</ul>}
      <div><button type="button" disabled={disabled || busy} onClick={() => void load(null)}>{t("m5.catalog.first")}</button>
        <button type="button" disabled={disabled || busy || !next || error} onClick={() => void load(next)}>{t("m5.catalog.next")}</button></div>
    </div>}
  </section>;
}
