import { useI18n } from "../i18n.tsx";
import { RecoveryGuide } from "./recovery-guide.tsx";
import { useEffect, useRef, useState } from "react";
import type { SupportedM5Bridge } from "./bridge-boundary.ts";
import type { M5BridgeV6 } from "./contracts/bridge-v6.ts";
import { summarizeWorkStatus, type WorkStatusSummary } from "./work-status.ts";

/**
 * Read-only status of work that still holds the app (a run to finish or to
 * confirm stopped). It lives at the top of the bell panel and reports whether
 * there is anything, so the bell can show a warning mark.
 * No automatic cancellation, lock release, or recovery mutation.
 */
export function WorkStatusPanel({ bridge, ready, refreshKey, onAttention, liveRunIds = [] }: {
  bridge: SupportedM5Bridge; ready: boolean; refreshKey: string; onAttention: (attention: boolean) => void;
  liveRunIds?: readonly string[];
}) {
  const { t } = useI18n();
  const [summary, setSummary] = useState<WorkStatusSummary | null>(null);
  const [failed, setFailed] = useState(false);
  const [busy, setBusy] = useState(false);
  const [refresh, setRefresh] = useState(0);
  const inFlight = useRef<Promise<unknown> | null>(null);
  useEffect(() => {
    let current = true;
    if (!ready || bridge.version < 6) { setSummary(null); setFailed(false); return; }
    setBusy(true);
    // Serialize behind an obsolete read; its result must never update this view.
    const previous = inFlight.current;
    const request = (async () => {
      if (previous) await previous.catch(() => {});
      if (!current) return;
      try {
        const status = await (bridge as M5BridgeV6).getSystemStatus();
        if (!current) return;
        setSummary(summarizeWorkStatus(status, liveRunIds)); setFailed(false);
      } catch { if (current) setFailed(true); }
      finally { if (current) setBusy(false); }
    })();
    inFlight.current = request;
    return () => { current = false; };
  }, [bridge, ready, refreshKey, refresh, liveRunIds.join(" ")]);
  const attention = ready && bridge.version >= 6 && (summary !== null || failed);
  useEffect(() => { onAttention(attention); }, [attention, onAttention]);
  if (!attention) return null;
  // F3 (ADE U7): the same F11 guide as other failures — what is going on, one
  // thing to do now, and the run list only inside the help details.
  const details = summary ? [t("m5.work.details", { count: summary.total }) + (failed ? t("m5.work.stale") : ""), t("m5.work.safety"), ...summary.ids,
    ...(summary.omitted > 0 ? [t("m5.work.omitted", { count: summary.omitted })] : [])].join("\n") : null;
  return <RecoveryGuide tone="attention" live={false} className="m5-bell-problem" label={t("m5.work.title")} busy={busy}
    title={failed ? t("m5.work.failed") : t("m5.work.active")}
    what={failed ? t("m5.work.error") : t("m5.work.help")}
    primary={{ label: busy ? t("m5.work.busy") : t("m5.work.refresh"), onClick: () => setRefresh(value => value + 1), disabled: busy }}
    details={details} />;
}
