import { useCallback, useState } from "react";
import { useI18n } from "../i18n.tsx";
import { ChatErasure } from "./chat-erasure.tsx";
import { WorkspaceCleanup } from "./workspace-cleanup.tsx";

/**
 * Settings → Storage: one card for everything left behind by discarded tasks.
 * Both halves keep their own selection, final confirmation and one-by-one
 * receipts; this card only shares the find button.
 */
export function DiscardedWorkCleanup({ disabled }: { disabled: boolean }) {
  const { t } = useI18n();
  const [loadKey, setLoadKey] = useState(0);
  const [filesBusy, setFilesBusy] = useState(false);
  const [contentBusy, setContentBusy] = useState(false);
  const onFilesBusy = useCallback((busy: boolean) => setFilesBusy(busy), []);
  const onContentBusy = useCallback((busy: boolean) => setContentBusy(busy), []);
  const busy = filesBusy || contentBusy;
  return <section className="m5-cleanup-settings" aria-busy={busy}>
    <h3>{t("m5.discardedWork.title")}</h3><p className="m5-settings-intro">{t("m5.discardedWork.help")}</p>
    <button type="button" disabled={disabled || busy} onClick={() => setLoadKey(key => key + 1)}>{t(busy ? "m5.discardedWork.busy" : "m5.discardedWork.inspect")}</button>
    <WorkspaceCleanup disabled={disabled} loadKey={loadKey} onBusy={onFilesBusy} />
    <ChatErasure disabled={disabled} loadKey={loadKey} onBusy={onContentBusy} />
  </section>;
}
