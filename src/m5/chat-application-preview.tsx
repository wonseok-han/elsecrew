import { useEffect, useRef, useState } from "react";
import { useI18n } from "../i18n.tsx";
import { validateProjectCatalogResult, type ApplicationPreview } from "./contracts/project-catalog-v1.ts";

export function ChatApplicationPreview({ projectId, chatId, worktreeId, disabled }: { projectId: string; chatId: string; worktreeId: string; disabled: boolean }) {
  const { t } = useI18n();
  const [preview, setPreview] = useState<ApplicationPreview | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState(false);
  const generation = useRef(0);
  const pending = useRef(false);
  useEffect(() => {
    generation.current++; setPreview(null); setError(false);
    return () => { generation.current++; };
  }, [projectId, chatId, worktreeId, disabled]);
  async function inspect() {
    if (disabled || pending.current || !window.m5Projects) return;
    const epoch = generation.current;
    pending.current = true; setBusy(true); setError(false); setPreview(null);
    const request = { action: "chatApplicationPreview" as const, projectId, chatId };
    try {
      const result = validateProjectCatalogResult(await window.m5Projects.request(request), request);
      if (generation.current !== epoch) return;
      if (result.action !== "chatApplicationPreview" || result.preview.worktreeId !== worktreeId) throw new Error("selected chat workspace changed");
      setPreview(result.preview);
    } catch { if (generation.current === epoch) setError(true); }
    finally { pending.current = false; setBusy(false); }
  }
  return <section aria-label={t("m5.application.chatPreview")} aria-busy={busy}>
    <button type="button" disabled={disabled || busy} onClick={() => void inspect()}>{t("m5.application.chatPreview")}</button>
    {error && <p role="status">{t("m5.application.previewError")}</p>}
    {preview && <>
      <p>{t("m5.application.previewHelp")}</p>
      <p>{t("m5.application.workspace")} <code>{preview.worktreeId}</code></p>
      <p>{t("m5.application.total")} {preview.total}</p>
      {preview.total === 0 && <p>{t("m5.application.noChanges")}</p>}
      {preview.total > preview.changes.length && <p>{t("m5.application.previewPartial")}</p>}
      <ul>{preview.changes.map(change => <li key={change.path}>{t(`m5.application.${change.kind}`)} <code>{change.path.replace(/[\p{Cc}\p{Cf}]/gu, character => `\\u{${character.codePointAt(0)!.toString(16)}}`)}</code></li>)}</ul>
    </>}
  </section>;
}
