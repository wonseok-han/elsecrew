// App update inside the app (ADE U8, Q7, mockup 16): a banner under the top bar
// that follows main's update state. The page only asks; main owns the state,
// starts a check, and alone confirms (in its own box) and performs a restart.
import { useEffect, useState } from "react";
import { useI18n } from "../i18n.tsx";

import { updateBannerKind, type UpdateState } from "./update-banner-kind.ts";

export function UpdateBanner({ teamWorking }: { teamWorking: boolean }) {
  const { t } = useI18n();
  const api = window.m5AppUpdate;
  const [state, setState] = useState<UpdateState>("unavailable");
  const [dismissed, setDismissed] = useState<UpdateState | null>(null);
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    if (!api?.state || !api.onState) return;
    let alive = true;
    void api.state().then(value => { if (alive) setState(value); }, () => undefined);
    const off = api.onState(value => setState(value));
    return () => { alive = false; off(); };
  }, [api]);
  const kind = updateBannerKind(state);
  if (!kind || dismissed === state || !api?.request) return null;
  const ask = (action: "check" | "restart") => {
    setBusy(true);
    void api.request!(action).then(value => setState(value), () => undefined).finally(() => setBusy(false));
  };
  const tone = kind === "error" ? "danger" : "running";
  return <section className={`m5-banner m5-banner-${tone} m5-update-banner`} role="status" aria-live="polite" data-testid="update-banner">
    <svg className="m5-update-banner-icon" viewBox="0 0 24 24" aria-hidden="true" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round"><path d="M12 4v11M7 10l5 5 5-5M5 20h14" /></svg>
    <div className="m5-update-banner-text">
      <strong>{t(`m5.update.${kind}.title`)}</strong>
      <p>{kind === "ready" && teamWorking ? t("m5.update.ready.waitTeam") : t(`m5.update.${kind}.help`)}</p>
    </div>
    <div className="m5-update-banner-actions">
      {kind === "ready" && <button type="button" className="m5-secondary" disabled={busy || teamWorking} onClick={() => ask("restart")}>{t("m5.update.restart")}</button>}
      {kind === "error" && <button type="button" className="m5-secondary" disabled={busy} onClick={() => ask("check")}>{t("m5.update.retry")}</button>}
      {kind === "ready" && <button type="button" className="m5-btn-quiet" onClick={() => setDismissed(state)}>{t("m5.update.later")}</button>}
      <button type="button" className="m5-btn-quiet m5-update-banner-close" aria-label={t("m5.notifications.dismiss")} title={t("m5.notifications.dismiss")} onClick={() => setDismissed(state)}>×</button>
    </div>
  </section>;
}
