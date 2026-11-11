import { createPortal } from "react-dom";
import { useCallback, useEffect, useRef, useState } from "react";
import { useI18n } from "../i18n";
import { validateInstallStatus, type InstallProvider, type InstallRequest, type InstallStatus } from "./contracts/provider-install-v1.ts";

const running = (status: InstallStatus | null) => status !== null &&
  ["starting", "downloading", "installing", "verifying", "stopping"].includes(status.state);

/** One installation manager is shared by both providers. Never replay a start on transport failure. */
export function ProviderInstall({ disabled, providers, onBlockChange, onCheck, onAttention, compact = false, integrated = false, slotsRevision = "" }: {
  disabled: boolean; providers: InstallProvider[]; onBlockChange: (blocked: boolean) => void;
  onCheck: (provider: InstallProvider) => void; compact?: boolean;
  integrated?: boolean; slotsRevision?: string;
  onAttention?: (attention: boolean) => void;
}) {
  const { t } = useI18n();
  const [slots, setSlots] = useState<Partial<Record<InstallProvider, HTMLElement>>>({});
  useEffect(() => { if (integrated) setSlots({codex: document.getElementById("codex-installation-slot")!, claude: document.getElementById("claude-installation-slot")!}); }, [integrated, slotsRevision]);
  const [status, setStatus] = useState<InstallStatus | null>(null);
  const [busy, setBusy] = useState(false);
  const [uncertain, setUncertain] = useState(false);
  const [consent, setConsent] = useState<string | null>(null);
  const [expanded, setExpanded] = useState(false);
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
    return () => { mounted.current = false; };
  }, []);
  useEffect(() => {
    // Initial storage verification rejects install requests. Wait for admission
    // instead of presenting that expected startup phase as an uncertain install.
    // Existing active/uncertain polling below still tracks owned operations.
    if (!disabled) void request({ action: "status" });
  }, [disabled, request]);
  const active = running(status);
  const poll = active || uncertain;
  useEffect(() => {
    if (!poll) return;
    const timer = window.setInterval(() => void request({ action: "status" }), 1500);
    return () => window.clearInterval(timer);
  }, [poll, request]);
  const plan = status?.plan;
  // Presentation only: keep the manager mounted and important actions visible.
  const actionRequired = busy || uncertain || active || Boolean(plan) || Boolean(status?.error) || status?.state === "recoveryRequired";
  const needsAttention = actionRequired || status?.state === "installed";
  useEffect(() => { onAttention?.(actionRequired); }, [actionRequired, onAttention]);
  if (!window.m5Install) return null;
  const showDetails = !compact || expanded || needsAttention;
  const canPrepare = !disabled && !busy && !uncertain && status !== null && !active &&
    !["unsupported", "recoveryRequired"].includes(status.state);
  const panel = <section data-active={active || undefined} className={`m5-provider-install${compact ? " m5-provider-install-compact" : ""}`} aria-label={t("m5.install.title")}>
    <header><div className="m5-install-heading"><strong>{t("m5.install.title")}</strong>
      <p role="status">{status?.provider && <strong>{status.provider === "codex" ? "Codex" : "Claude Code"} · </strong>}
        {status ? t(`m5.install.state.${status.state}`) : t("m5.install.checking")}</p></div>
      <div className="m5-install-header-actions">
    {active && status?.operationId && <button className="m5-install-stop" type="button" disabled={busy || uncertain || status.cancelRequested || status.state === "stopping"}
      onClick={() => void request({ action: "cancel", operationId: status.operationId! })}>{t(status.cancelRequested ? "m5.install.canceling" : "m5.install.cancel")}</button>}
      {showDetails && <button className="m5-install-refresh" type="button" disabled={busy} aria-label={t("m5.install.refresh")} title={t("m5.install.refresh")} onClick={() => void request({ action: "status" })}><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.7" aria-hidden="true"><path d="M20 7v5h-5M20 12a8 8 0 1 0-2 5M20 7l-3 3"/></svg></button>}
      {compact && !active && <button type="button" aria-expanded={showDetails} aria-controls="m5-install-details" disabled={needsAttention}
        onClick={() => setExpanded(value => !value)}>{t(showDetails ? "m5.install.hideDetails" : "m5.install.showDetails")}</button>}
      </div>
    </header>
    <div id="m5-install-details" hidden={!showDetails}>
    {uncertain && <p role="alert">{t("m5.install.uncertain")}</p>}
    {!integrated && !active && <div className="m5-install-actions m5-install-provider-actions">{providers.map(provider => <button key={provider} type="button" disabled={!canPrepare}
      onClick={() => void request({ action: "prepare", provider })}>{t("m5.install.prepare", { name: provider === "codex" ? "Codex" : "Claude Code" })}</button>)}</div>}
    {plan && !uncertain && !active && <form onSubmit={event => {
      event.preventDefault();
      if (!disabled && !busy && consent === plan.planId) void request({ action: "start", provider: plan.provider, planId: plan.planId });
    }}>
      <p>{t("m5.install.scope")}</p>
      <details className="m5-install-paths"><summary>{t("m5.install.locations")}</summary><ul>{plan.writeDirectories.map(path => <li key={path}><code><bdi>{path}</bdi></code></li>)}</ul></details>
      <p>{t("m5.install.expiry")}</p>
      <label><input type="checkbox" disabled={disabled || busy} checked={consent === plan.planId}
        onChange={event => setConsent(event.currentTarget.checked ? plan.planId : null)}/>{t("m5.install.consent")}</label>
      <div className="m5-install-actions"><button type="submit" disabled={disabled || busy || consent !== plan.planId}>{t("m5.install.start")}</button>
        <button type="button" disabled={busy} onClick={() => void request({ action: "dismiss" })}>{t("m5.install.dismiss")}</button></div>
    </form>}

    {status?.state === "installed" && status.provider && !uncertain && <button type="button" disabled={disabled || busy}
      onClick={() => onCheck(status.provider!)}>{t("m5.install.checkLogin")}</button>}
    {status?.state === "recoveryRequired" && !uncertain && <div className="m5-install-actions">
      <button type="button" disabled={disabled || busy} onClick={() => void request({ action: "inspectRecovery" })}>{t("m5.install.inspectRecovery")}</button>
      {status.operationId && <button type="button" disabled={disabled || busy}
        onClick={() => void request({ action: "recover", operationId: status.operationId! })}>{t("m5.install.recover")}</button>}
      <p>{t("m5.install.recoveryScope")}</p>
    </div>}
    {status?.error && <details><summary>{t("m5.install.details")}</summary><code>{status.error}</code></details>}
    </div>
  </section>;
  if (!integrated) return panel;
  return <>{providers.map(provider => {
    const slot = slots[provider];
    if (!slot) return null;
    const ownsDetails = status?.provider === provider && (active || Boolean(plan) || Boolean(status.error) || status.state === "installed" || status.state === "recoveryRequired");
    return createPortal(ownsDetails ? panel : <div className="m5-provider-install-inline">
      <button type="button" disabled={!canPrepare} onClick={() => void request({action:"prepare",provider})}>{t("m5.install.manage")}</button>
      <button className="m5-install-refresh" type="button" disabled={busy} aria-label={t("m5.install.refresh")} title={t("m5.install.refresh")} onClick={() => void request({action:"status"})}>↻</button>
      {uncertain && <p role="alert">{t("m5.install.uncertain")}</p>}
    </div>, slot, provider);
  })}</>;
}
