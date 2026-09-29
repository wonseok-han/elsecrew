import { useEffect, useRef, useState } from "react";
import type { ProjectEditorController } from "./editor-state.ts";
import { validateProjectCatalogResult, type ProjectCatalogEntry } from "./contracts/project-catalog-v1.ts";

export function ProjectCatalog({ controller, disabled, currentId }: {
  controller: ProjectEditorController; disabled: boolean; currentId?: string;
}) {
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
  return <section className="m5-project-catalog" aria-label="저장된 프로젝트">
    <button type="button" className="m5-secondary" disabled={disabled || busy} onClick={() => open ? setOpen(false) : void load(null)} aria-expanded={open}>저장된 프로젝트</button>
    {open && <div className="m5-project-catalog-list" aria-busy={busy}>
      <p>전에 열었던 프로젝트 · ID 순서</p>
      {busy && <p role="status">프로젝트를 확인하고 있어요…</p>}
      {error ? <p role="alert">목록을 불러오지 못했어요. 다시 확인해 주세요.</p> : !busy && entries.length === 0 ? <p>저장된 프로젝트가 없어요. 프로젝트 선택으로 폴더를 열어 주세요.</p> :
        <ul>{entries.map(entry => <li key={entry.projectId}><button type="button" disabled={disabled || busy || entry.projectId === currentId} onClick={() => void select(entry.projectId)}>
          <strong>{entry.canonicalPath.split(/[\\/]/).filter(Boolean).pop() ?? entry.projectId}{entry.projectId === currentId ? " · 현재 프로젝트" : ""}</strong>
          <span dir="auto">{entry.canonicalPath}</span>
        </button></li>)}</ul>}
      <div><button type="button" disabled={disabled || busy} onClick={() => void load(null)}>처음부터 다시 확인</button>
        <button type="button" disabled={disabled || busy || !next || error} onClick={() => void load(next)}>다음 목록</button></div>
    </div>}
  </section>;
}
