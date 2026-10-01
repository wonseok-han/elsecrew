import { useEffect, useRef, useState } from "react";
import { useI18n } from "../i18n.tsx";
import { ApplicationStartControl } from "./application-start-control.tsx";
import { validateProjectCatalogResult, type ApplicationPreviewPage } from "./contracts/project-catalog-v1.ts";

export function RestorationPreview({ projectId, applicationId, disabled }: { projectId: string; applicationId: string; disabled: boolean }) {
  const { t } = useI18n();
  const [page, setPage] = useState<ApplicationPreviewPage | null>(null);
  const [busy, setBusy] = useState(false);
  const [failed, setFailed] = useState(false);
  const epoch = useRef(0);
  const pending = useRef(false);
  useEffect(() => {
    epoch.current++; setPage(null); setFailed(false);
    return () => { epoch.current++; };
  }, [projectId, applicationId, disabled]);
  async function load(offset = 0, fingerprint: string | null = null) {
    if (disabled || pending.current || !window.m5Projects) return;
    const generation = epoch.current;
    pending.current = true; setBusy(true); setFailed(false); setPage(null);
    const request = { action: "restorationPreview" as const, projectId, applicationId, offset, fingerprint };
    try {
      const result = validateProjectCatalogResult(await window.m5Projects.request(request), request);
      if (epoch.current === generation && result.action === "restorationPreview") setPage(result.page);
    } catch {
      if (epoch.current === generation) setFailed(true);
    } finally { pending.current = false; setBusy(false); }
  }
  return <section aria-label={t("m5.restore.preview")} aria-busy={busy}>
    <button type="button" disabled={disabled || busy} onClick={() => void load()}>{t("m5.restore.preview")}</button>
    {failed && <p role="status">{t("m5.restore.error")}</p>}
    {page && <>
      <p>{t("m5.restore.help")}</p>
      <p>{t("m5.application.total")} {page.total} · {page.total === 0 ? 0 : page.offset + 1}–{page.offset + page.changes.length}</p>
      <ul>{page.changes.map(change => <li key={change.path}>
        {t(`m5.application.${change.kind}`)} <code>{change.path.replace(/[\p{Cc}\p{Cf}]/gu, character => `\\u{${character.codePointAt(0)!.toString(16)}}`)}</code>
      </li>)}</ul>
      <button type="button" disabled={disabled || busy || page.offset === 0} onClick={() => void load(page.offset - 100, page.fingerprint)}>{t("m5.restore.previous")}</button>
      <button type="button" disabled={disabled || busy || page.offset + page.changes.length >= page.total} onClick={() => void load(page.offset + 100, page.fingerprint)}>{t("m5.restore.next")}</button>
      <ApplicationStartControl key={page.fingerprint} request={{action:"restoreApplication",projectId,applicationId,fingerprint:page.fingerprint}} disabled={disabled || busy} />
    </>}
  </section>;
}
