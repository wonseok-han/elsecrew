import { useCallback, useEffect, useRef, useState } from "react";
import { useI18n } from "../i18n";
import { validateInstallStatus, type InstallProvider, type InstallRequest, type InstallStatus } from "./contracts/provider-install-v1.ts";

const running = (status: InstallStatus | null) => status !== null &&
  ["starting", "downloading", "installing", "verifying", "stopping"].includes(status.state);

/** One installation manager is shared by both providers. Never replay a start on transport failure. */
export function ProviderInstall({ disabled, providers, onBlockChange, onCheck }: {
  disabled: boolean; providers: InstallProvider[]; onBlockChange: (blocked: boolean) => void;
  onCheck: (provider: InstallProvider) => void;
}) {
  const { t } = useI18n();
  const [status, setStatus] = useState<InstallStatus | null>(null);
  const [busy, setBusy] = useState(false);
  const [uncertain, setUncertain] = useState(false);
  const [consent, setConsent] = useState<string | null>(null);
  const mounted = useRef(false);
  const inFlight = useRef(false);
  const request = useCallback(async (input: InstallRequest) => {
    if (!window.m5Install || inFlight.current || !mounted.current) return;
    inFlight.current = true;
    setBusy(true);
    if (input.action !== "status") onBlockChange(true);
    try {
      const next = validateInstallStatus(await window.m5Install.request(input));
      if (!mounted.current) return;
      setStatus(next);
      setUncertain(false);
      // A new plan always needs new consent, including after an ambiguous reply.
      if (input.action !== "status") setConsent(null);
      onBlockChange(running(next) || next.state === "recoveryRequired" || next.state === "awaitingConsent");
    } catch {
      if (mounted.current) { setUncertain(true); setConsent(null); onBlockChange(true); }
    } finally {
      inFlight.current = false;
      if (mounted.current) setBusy(false);
    }
  }, [onBlockChange]);
  useEffect(() => {
    mounted.current = true;
    void request({ action: "status" });
    return () => { mounted.current = false; };
  }, [request]);
  const active = running(status);
  const poll = active || uncertain;
  useEffect(() => {
    if (!poll) return;
    const timer = window.setInterval(() => void request({ action: "status" }), 1500);
    return () => window.clearInterval(timer);
  }, [poll, request]);
  if (!window.m5Install) return null;
  const plan = status?.plan;
  const canPrepare = !disabled && !busy && !uncertain && status !== null && !active &&
    !["unsupported", "recoveryRequired"].includes(status.state);
  return <section className="m5-provider-install" aria-label={t("m5.install.title")}>
    <header><strong>{t("m5.install.title")}</strong>
      <button type="button" disabled={busy} onClick={() => void request({ action: "status" })}>{t("m5.install.refresh")}</button>
    </header>
    <p role="status">{status?.provider && <strong>{status.provider === "codex" ? "Codex" : "Claude Code"} · </strong>}
      {status ? t(`m5.install.state.${status.state}`) : t("m5.install.checking")}</p>
    {uncertain && <p role="alert">{t("m5.install.uncertain")}</p>}
    <div className="m5-install-actions">{providers.map(provider => <button key={provider} type="button" disabled={!canPrepare}
      onClick={() => void request({ action: "prepare", provider })}>{t("m5.install.prepare", { name: provider === "codex" ? "Codex" : "Claude Code" })}</button>)}</div>
    {plan && !uncertain && <form onSubmit={event => {
      event.preventDefault();
      if (!disabled && !busy && consent === plan.planId) void request({ action: "start", provider: plan.provider, planId: plan.planId });
    }}>
      <p>{t("m5.install.scope")}</p>
      <ul>{plan.writeDirectories.map(path => <li key={path}><code><bdi>{path}</bdi></code></li>)}</ul>
      <p>{t("m5.install.expiry")}</p>
      <label><input type="checkbox" disabled={disabled || busy} checked={consent === plan.planId}
        onChange={event => setConsent(event.currentTarget.checked ? plan.planId : null)}/>{t("m5.install.consent")}</label>
      <div className="m5-install-actions"><button type="submit" disabled={disabled || busy || consent !== plan.planId}>{t("m5.install.start")}</button>
        <button type="button" disabled={busy} onClick={() => void request({ action: "dismiss" })}>{t("m5.install.dismiss")}</button></div>
    </form>}
    {active && status?.operationId && <button type="button" disabled={busy || uncertain || status.cancelRequested || status.state === "stopping"}
      onClick={() => void request({ action: "cancel", operationId: status.operationId! })}>{t(status.cancelRequested ? "m5.install.canceling" : "m5.install.cancel")}</button>}
    {status?.state === "installed" && status.provider && !uncertain && <button type="button" disabled={disabled || busy}
      onClick={() => onCheck(status.provider!)}>{t("m5.install.checkLogin")}</button>}
    {status?.error && <details><summary>{t("m5.install.details")}</summary><code>{status.error}</code></details>}
  </section>;
}
