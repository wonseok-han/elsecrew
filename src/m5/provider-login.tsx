import { loadModels } from "./model-catalog.ts";
import { readConnections, changeConnection, CONNECTIONS_CHANGED, PROVIDER_AUTH_CHANGED } from "./provider-connections.ts";
import { useCallback, useEffect, useRef, useState } from "react";
import { useI18n } from "../i18n";
import { loginErrorMessage } from "./provider-login-message.ts";
import { LOGIN_PROVIDERS, readProviderSelection, saveProviderSelection } from "./provider-selection.ts";
import { validateLoginStatus, type LoginRequest, type LoginStatus, type LoginProvider } from "./contracts/provider-login-v1.ts";
import { validateDeviceReport, type DeviceReport } from "./contracts/device-report-v1.ts";
import { ProviderInstall, INSTALL_STATUS_REFRESH, INSTALL_PREPARE } from "./provider-install.tsx";
import { loginGuide, type LoginGuideAction } from "./login-guide.ts";
import { RecoveryGuide, type GuideAction } from "./recovery-guide.tsx";
import { shouldPollLogin } from "./provider-login-poll.ts";
import { readLoginDiagnostic, type LoginDiagnostic } from "./provider-login-diagnostic.ts";

export type ConnectionReveal = { provider: LoginProvider; sequence: number };

export function ProviderLogin({ disabled, compact: preferCompact = false, returning = false, settingsPage = false, reveal, onSwitchRoles }: {disabled: boolean; compact?: boolean; returning?: boolean; settingsPage?: boolean; reveal?: ConnectionReveal;
  /** Opens the team roles to move this agent's roles to another agent (ADE U5 shortcut). */
  onSwitchRoles?: (provider: LoginProvider) => void}) {
  const { t } = useI18n();
  const [selected, setSelected] = useState(readProviderSelection);
  const [startedWithoutAgents] = useState(() => readProviderSelection().length === 0);
  const compact = !settingsPage && (preferCompact || (returning && !startedWithoutAgents));
  const SelectionContainer = settingsPage ? "section" : "details";
  const [selectionExpanded, setSelectionExpanded] = useState(!settingsPage && !compact && selected.length === 0);
  useEffect(() => { setSelectionExpanded(!settingsPage && !compact && selected.length === 0); }, [settingsPage, compact, selected.length]);
  const [draft, setDraft] = useState<LoginProvider[]>([]);
  const [saveFailed, setSaveFailed] = useState(false);
  const [installBlocked, setInstallBlocked] = useState(Boolean(window.m5Install));
  const [installAttention, setInstallAttention] = useState(false);
  const [managementExpanded, setManagementExpanded] = useState(false);
  const [checks, setChecks] = useState({ codex: 0, claude: 0 });
  const [step, setStep] = useState<"agents" | "install" | "login">("agents");
  const [loginAttention, setLoginAttention] = useState({ codex: false, claude: false });
  const reportLoginAttention = useCallback((provider: LoginProvider, attention: boolean) => {
    setLoginAttention(current => current[provider] === attention ? current : { ...current, [provider]: attention });
  }, []);
  const loginNeedsAttention = selected.some(provider => loginAttention[provider]);
  const needsAttention = installBlocked || installAttention || loginNeedsAttention || saveFailed;
  const showManagement = !compact || managementExpanded || needsAttention;
  const managementRef = useRef<HTMLElement>(null);
  const revealedSequence = useRef<number | null>(null);
  useEffect(() => {
    if (!reveal || revealedSequence.current === reveal.sequence) return;
    revealedSequence.current = reveal.sequence;
    setManagementExpanded(true);
    if (!selected.includes(reveal.provider)) setSelectionExpanded(true);
    requestAnimationFrame(() => {
      if (revealedSequence.current !== reveal.sequence) return;
      managementRef.current?.scrollIntoView({ block: "start" });
      managementRef.current?.focus({ preventScroll: true });
    });
  }, [reveal, selected]);
  const navigate = (next: typeof step) => { if (!installBlocked) setStep(next); };
  const select = (providers: LoginProvider[]) => {
    if (disabled) return;
    setSaveFailed(!saveProviderSelection(providers));
    setSelected(providers);
    setSelectionExpanded(false);
    setStep("install");
  };
  return <section ref={managementRef} tabIndex={-1} aria-label={t("m5.setup.management")} className={compact ? "m5-agent-management m5-agent-management-compact" : "m5-agent-management"}>
    {compact && <button className="m5-management-toggle" type="button" aria-expanded={showManagement} aria-controls="m5-agent-management-content" disabled={needsAttention} onClick={() => setManagementExpanded(value => !value)}>
      {t("m5.setup.management")}<span>{t(needsAttention ? "m5.setup.managementAttention" : showManagement ? "m5.setup.managementHide" : "m5.setup.managementShow")}</span>
    </button>}
    <div id="m5-agent-management-content" hidden={!showManagement}>
    {reveal && <p className="m5-notice">{t("m5.setup.requestedConnection", { name: reveal.provider === "codex" ? "Codex" : "Claude Code" })}</p>}
    {!settingsPage && !compact && <nav className="m5-setup-navigation" aria-label={t("m5.setup.steps.label")}>
      {(["agents", "install", "login"] as const).map((item, index) => <button type="button" key={item}
        aria-current={step === item ? "step" : undefined}
        disabled={installBlocked || (item !== "agents" && selected.length === 0)}
        onClick={() => navigate(item)}><span aria-hidden="true">{index + 1}</span>{t(`m5.setup.steps.${item}`)}</button>)}
    </nav>}
    <div hidden={!settingsPage && !compact && step !== "agents"}>
    {/* Device facts help the first setup; settings keeps only what can be changed. */}
    {!settingsPage && <DeviceCheck/>}
    {/* Settings already lists every agent with its own connect button; the
        guided-setup selection would only add a second, unsynced choice. */}
    {!settingsPage && <SelectionContainer className="m5-provider-selection" open={selectionExpanded} onToggle={event => setSelectionExpanded(event.currentTarget.open)}>
      {settingsPage ? <h4>{t("m5.setup.agents.title")}</h4> : <summary><strong>{t("m5.setup.agents.title")}</strong><span>{selected.length ? selected.map(p => p === "codex" ? "Codex" : "Claude Code").join(" · ") : t("m5.setup.agents.choose")}</span></summary>}
      <p>{t("m5.setup.agents.help")}</p>
      {selected.length === 0 ? <form onSubmit={event => { event.preventDefault(); if (draft.length) select(draft); }}>
        <fieldset disabled={disabled}>
          <legend>{t("m5.setup.agents.choose")}</legend>
          {LOGIN_PROVIDERS.map(provider => <label key={provider}>
            <input type="checkbox" checked={draft.includes(provider)} onChange={event => { const checked = event.currentTarget.checked; setDraft(current => checked ? [...current,provider] : current.filter(item => item !== provider)); }}/>
            {provider === "codex" ? "Codex" : "Claude Code"}
          </label>)}
        </fieldset>
        <button type="submit" disabled={disabled || draft.length === 0}>{t("m5.setup.agents.continue")}</button>
      </form> : <div>
        <p>{t("m5.setup.agents.selected", {names: selected.map(p => p === "codex" ? "Codex" : "Claude Code").join(", ")})}</p>
        {LOGIN_PROVIDERS.filter(p => !selected.includes(p)).map(provider => <button key={provider} type="button" disabled={disabled} onClick={() => select([...selected,provider])}>{t("m5.setup.agents.add", {name: provider === "codex" ? "Codex" : "Claude Code"})}</button>)}
      </div>}
      {saveFailed && <p role="status">{t("m5.setup.agents.saveFailed")}</p>}
    </SelectionContainer>}
    {!settingsPage && !compact && selected.length > 0 && <button type="button" className="m5-setup-next" disabled={installBlocked} onClick={() => navigate("install")}>{t("m5.setup.steps.toInstall")}</button>}
    </div>
    {!selectionExpanded && saveFailed && <p className="m5-notice" role="status">{t("m5.setup.agents.saveFailed")}</p>}
    <div hidden={!settingsPage && !compact && step !== "install" && !installBlocked}>
    <ProviderInstall slotsRevision={`${checks.codex}:${checks.claude}`} integrated={settingsPage} disabled={disabled} providers={settingsPage ? [...LOGIN_PROVIDERS] : selected} compact={compact} onBlockChange={setInstallBlocked} onAttention={setInstallAttention}
      onCheck={provider => { setChecks(current => ({ ...current, [provider]: current[provider] + 1 })); setStep("login"); }}/>
    {!settingsPage && !compact && <div className="m5-setup-next"><p>{t("m5.setup.steps.installHelp")}</p><button type="button" disabled={installBlocked || selected.length === 0} onClick={() => navigate("login")}>{t("m5.setup.steps.toLogin")}</button></div>}
    </div>
    <div hidden={!settingsPage && !compact && step !== "login" && !loginNeedsAttention}>
    {settingsPage && <p className="m5-settings-intro">{t("m5.login.settingsIntro")}</p>}
    {LOGIN_PROVIDERS.filter(provider => settingsPage || selected.includes(provider)).map(provider => <ProviderConnection key={`${provider}:${checks[provider]}`} disabled={disabled || installBlocked} provider={provider} alwaysExpanded={settingsPage} onAttention={reportLoginAttention} onSwitchRoles={onSwitchRoles}/>)}
    {!settingsPage && !compact && <p className="m5-setup-next">{t("m5.setup.steps.projectHelp")}</p>}
    </div>
    </div>
  </section>;
}
function DeviceCheck({ expanded = false }: { expanded?: boolean }) {
  const Container = expanded ? "section" : "details";
  const { lang, t } = useI18n();
  const [report, setReport] = useState<DeviceReport | null>(null);
  const [busy, setBusy] = useState(false);
  const [failed, setFailed] = useState(false);
  const mounted = useRef(false);
  const inFlight = useRef(false);
  const inspect = async () => {
    if (!window.m5Device || inFlight.current) return;
    inFlight.current = true;
    setBusy(true); setFailed(false);
    try {
      const value = validateDeviceReport(await window.m5Device.inspect());
      if (mounted.current) setReport(value);
    } catch { if (mounted.current) { setReport(null); setFailed(true); } }
    finally { inFlight.current = false; if (mounted.current) setBusy(false); }
  };
  useEffect(() => {
    mounted.current = true;
    void inspect();
    return () => { mounted.current = false; };
  }, []);
  if (!window.m5Device) return null;
  const capacity = (bytes: number) => `${new Intl.NumberFormat(lang, { maximumFractionDigits: 1 }).format(bytes / 1024 ** 3)} GiB`;
  return <Container className="m5-device-check" open={failed || undefined}>
    {expanded ? <h4>{t("m5.device.title")}</h4> : <summary>{t("m5.device.title")}</summary>}
    {busy && <p role="status">{t("m5.device.checking")}</p>}
    {failed && <p role="alert">{t("m5.device.failed")}</p>}
    {report && <dl>
      <div><dt>{t("m5.device.os")}</dt><dd>{report.platform === "darwin" ? "macOS" : report.platform === "win32" ? "Windows" : t("m5.device.other")} · {report.osVersion}</dd></div>
      <div><dt>{t("m5.device.cpu")}</dt><dd>{report.arch === "other" ? t("m5.device.other") : report.arch}</dd></div>
      <div><dt>{t("m5.device.memory")}</dt><dd>{capacity(report.memoryBytes)}</dd></div>
      <div><dt>{t("m5.device.space")}</dt><dd>{report.availableBytes === null ? t("m5.device.unknown") : capacity(report.availableBytes)}</dd></div>
    </dl>}
    <p>{t("m5.device.scope")}</p>
    <button type="button" disabled={busy} onClick={() => void inspect()}>{t("m5.device.refresh")}</button>
  </Container>;
}
function ProviderConnection({ disabled, provider, alwaysExpanded = false, onAttention, onSwitchRoles }: {alwaysExpanded?: boolean; disabled: boolean; provider: LoginProvider; onAttention: (provider: LoginProvider, attention: boolean) => void; onSwitchRoles?: (provider: LoginProvider) => void}) {
  const { lang, t } = useI18n();
  const name = provider === "codex" ? "Codex" : "Claude Code";
  const [appConnected,setAppConnected] = useState<boolean | null>(null);
  const [connectionBusy,setConnectionBusy] = useState(false);
  const [connectionError,setConnectionError] = useState(false);
  const runtime = provider === "codex" ? "codex" : "claude-code";
  useEffect(() => {
    if (disabled) return;
    let alive = true;
    const load = () => { void readConnections().then(value => { if(alive) { setAppConnected(value[runtime]); setConnectionError(false); } }).catch(() => { if(alive) setConnectionError(true); }); };
    load(); window.addEventListener(CONNECTIONS_CHANGED,load);
    return () => { alive=false; window.removeEventListener(CONNECTIONS_CHANGED,load); };
  },[runtime,disabled]);
  const toggleConnection = async () => {
    if(connectionBusy || appConnected === null) return;
    setConnectionBusy(true); setConnectionError(false);
    const connecting = !appConnected;
    try {
      const value=await changeConnection(runtime,!appConnected);
      // Connecting is one step for the user: if the account check that follows
      // finds no app login, the browser sign-in starts automatically.
      loginAfterConnect.current = connecting && value[runtime] === true;
      setAppConnected(value[runtime]);
    }
    catch { setConnectionError(true); }
    finally {setConnectionBusy(false);}
  };
  const [status,setStatus] = useState<LoginStatus>({state:"idle",attemptId:null,authUrl:null,error:null});
  const [busy,setBusy] = useState(false);
  const [error,setError] = useState(false);
  const [diagnostic,setDiagnostic] = useState<LoginDiagnostic | null>(null);
  const [expanded,setExpanded] = useState(false);
  const inFlight = useRef(false);
  const loginAfterConnect = useRef(false);
  const busyRetries = useRef(0);
  const busyRetryAttempt = useRef<string | null>(null);
  const checkAttempt = useRef<string | null>(null);
  const mounted = useRef(true);
  const checked = useRef(false);
  const recoveryCheck = useRef(false);
  const [code,setCode] = useState("");
  const [codeError,setCodeError] = useState(false);
  const scheduleBusyRetry = () => {
    window.setTimeout(() => {
      if (!mounted.current) return;
      if (inFlight.current) { scheduleBusyRetry(); return; }
      const attemptId = crypto.randomUUID();
      checkAttempt.current = attemptId;
      void request({action:"check",attemptId}, true);
    }, 1500);
  };
  const request = async (input: LoginRequest, busyRetry = false) => {
    if (inFlight.current || !window.m5Login) return;
    if (input.action === "check" && !busyRetry) { busyRetries.current = 0; checkAttempt.current = input.attemptId; }
    inFlight.current = true;
    setBusy(true);
    try {
      const scoped: LoginRequest = input.action === "submitCode" ? input : {...input,provider};
      if (input.action === "check") recoveryCheck.current = true;
      if (input.action === "start" || input.action === "cancel" || input.action === "diagnose") recoveryCheck.current = false;
      let result = validateLoginStatus(await window.m5Login.request(scoped),provider);
      // One verified cleanup per account check. Recovery never starts a login
      // or bypasses the backend's termination/ledger evidence requirements.
      if (recoveryCheck.current && result.error === "providerLoginRecoveryRequired") {
        recoveryCheck.current = false;
        result = validateLoginStatus(await window.m5Login.request({action:"recover",provider,attemptId:crypto.randomUUID()}),provider);
        if (result.state === "idle" && !result.error) {
          result = validateLoginStatus(await window.m5Login.request({action:"check",provider,attemptId:crypto.randomUUID()}),provider);
        }
      } else if (!["starting","waiting","verifying"].includes(result.state)) recoveryCheck.current = false;
      // Another check (for example the usage model query) briefly owns the same
      // provider profile. That is retryable contention, never recovery evidence.
      // Only account checks are retried; a busy login start waits for the user.
      if (input.action !== "cancel" && result.state === "failed" && result.error === "providerBusy"
        && result.attemptId === checkAttempt.current && result.attemptId !== busyRetryAttempt.current && busyRetries.current < 5) {
        busyRetries.current += 1;
        busyRetryAttempt.current = result.attemptId;
        scheduleBusyRetry();
      }
      if(mounted.current) { setStatus(result); setError(false); setDiagnostic(null); }
    } catch (failure) { if(mounted.current) { setError(true); setDiagnostic(readLoginDiagnostic(failure)); } }
    finally { inFlight.current=false; if(mounted.current) setBusy(false); }
  };
  useEffect(() => {
    mounted.current=true;
    if(disabled || appConnected === null) return;
    if (!checked.current) {
      checked.current=true;
      void request({action:"diagnose",attemptId:crypto.randomUUID()});
    } else if (status.state !== "installed") void request({action:"status"});
    return () => { mounted.current=false; };
  },[disabled,appConnected]);
  const poll = shouldPollLogin(status.state, error, disabled || (appConnected !== true && status.state !== "diagnosing"));
  useEffect(() => {
    if (!poll) return;
    const timer = window.setInterval(() => void request({action:"status"}), 500);
    return () => window.clearInterval(timer);
  }, [poll]);
  useEffect(() => {
    // Installation is not authentication. Only the existing account check can
    // report a connected account; diagnosis never reads credentials.
    if (!disabled && appConnected === true && status.state === "installed") {
      void request({action:"check",attemptId:crypto.randomUUID()});
    }
  },[disabled,appConnected,status.state]);
  useEffect(() => {
    if (!loginAfterConnect.current || disabled || appConnected !== true) return;
    if (status.state === "idle" && status.attemptId !== null) {
      loginAfterConnect.current = false;
      // Let the finished check release its worker slot before starting login.
      window.setTimeout(() => { if (mounted.current) void request({action:"start",attemptId:crypto.randomUUID()}); }, 300);
      return;
    }
    // A busy check is retried and may still end idle; keep waiting for it.
    if (status.state === "failed" && status.error === "providerBusy") return;
    if (["connected","failed","unsupported","canceled"].includes(status.state)) loginAfterConnect.current = false;
  },[disabled,appConnected,status.state,status.attemptId]);
  useEffect(()=>{setCode("");setCodeError(false);},[status.attemptId,status.state]);
  useEffect(()=>{ if (status.state === "connected") {
    setExpanded(false);
    window.dispatchEvent(new Event(PROVIDER_AUTH_CHANGED));
    // Keep the display-only model list ready for role screens; a fresh saved
    // list is reused, so this does not query the provider on every check.
    if (window.m5Models) void loadModels(runtime, input => window.m5Models!.request(input)).catch(() => undefined);
  } },[status.state]);
  const waiting=status.state === "diagnosing" || status.state === "starting" || status.state === "waiting" || status.state === "verifying";
  const recoveryRequired=status.error === "providerLoginRecoveryRequired";
  const profileCompatibilityRequired=status.error === "providerProfileIsolationRequired";
  useEffect(() => {
    onAttention(provider, waiting || recoveryRequired || error || status.state === "failed");
  }, [provider, waiting, recoveryRequired, error, status.state, onAttention]);
  const canCollapse=!alwaysExpanded && status.state === "connected" && !status.error && !error;
  const showDetails=!canCollapse || expanded;
  const label = appConnected === true ? t(`m5.login.state.${status.state}`, { name }) : t("m5.connection.off");
  // ADE U5 (mockup 08): a card per agent with its state as a badge, the actions
  // in one row, and a failure told as an F11 guide (what happened, one thing
  // to do now, alternatives, help details) instead of a long sentence.
  const failed = error || status.state === "failed" || recoveryRequired;
  const badge = appConnected !== true ? { tone: "", text: t("m5.connection.off") }
    : failed ? { tone: "m5-badge-attention", text: t("m5.login.badge.attention") }
    : waiting ? { tone: "m5-badge-running", text: t("m5.login.badge.checking") }
    : status.state === "connected" ? { tone: "m5-badge-done", text: t("m5.login.connected") }
    : { tone: "", text: t("m5.login.badge.signin") };
  const check = () => { window.dispatchEvent(new Event(INSTALL_STATUS_REFRESH)); void request({action:appConnected ? "check" : "diagnose",attemptId:crypto.randomUUID()}); };
  const checkDisabled = disabled||busy||waiting||!window.m5Login;
  const loginDisabled = disabled||busy||recoveryRequired||profileCompatibilityRequired||status.state==="connected"||status.state==="unsupported"||!window.m5Login;
  const other = provider === "codex" ? "Claude Code" : "Codex";
  const guideAction = (action: LoginGuideAction): GuideAction | null => {
    switch (action) {
      // The same action as the card's check button, under the same name; the card hides its own while a guide shows (minor 3).
      case "check": return { label: t("m5.login.check"), ariaLabel: t("m5.login.check"), onClick: check, disabled: checkDisabled };
      case "retryLogin": return { label: t("m5.login.guide.action.retryLogin"), ariaLabel: t("m5.login.guide.action.retryLoginNamed", { name }), onClick: () => void request({action:"start",attemptId:crypto.randomUUID()}), disabled: loginDisabled };
      case "install": return window.m5Install ? { label: t("m5.login.guide.action.install"), ariaLabel: t("m5.login.guide.action.installNamed", { name }), onClick: () => window.dispatchEvent(new CustomEvent(INSTALL_PREPARE, { detail: provider })), disabled } : null;
      // It opens the team roles, where the person moves roles to another agent and picks its model (minor 2).
      case "switchRoles": return onSwitchRoles ? { label: t("m5.login.guide.action.switchRoles"), ariaLabel: t("m5.login.guide.action.switchRolesNamed", { other }), onClick: () => onSwitchRoles(provider) } : null;
    }
  };
  const guide = appConnected === true && failed ? loginGuide(provider, error ? diagnostic ?? "providerLoginTransportUnavailable" : status.error) : null;
  return <section className="m5-provider-login m5-card" aria-label={t("m5.login.section", { name })} data-provider={provider}>
    <header className="m5-connection-heading">
      <strong>{name}</strong><span className={`m5-badge ${badge.tone}`.trim()}>{badge.text}</span>
      {["installed","connected"].includes(status.state) && <small>{t("m5.install.detected")}</small>}
      <button className="m5-connection-toggle m5-btn-quiet" type="button" aria-label={t(appConnected ? "m5.connection.disconnectNamed" : "m5.connection.connectNamed", { name })} disabled={disabled || connectionBusy || appConnected === null || waiting} onClick={()=>void toggleConnection()}>{t(appConnected ? "m5.connection.disconnect" : "m5.connection.connect")}</button>
    </header>
    <p role="status" className="m5-connection-state">{label}</p>
    {/* Settings explains this once above both agents instead of on each card. */}
    {!alwaysExpanded && <small className="m5-connection-help">{t("m5.login.separate")}</small>}
    {connectionError && <div className="m5-connection-error m5-banner m5-banner-danger" role="alert"><span>{t(appConnected === null ? "m5.connection.loadFailed" : "m5.connection.failed")}</span>{appConnected === null && <button type="button" className="m5-secondary" onClick={() => window.dispatchEvent(new Event(CONNECTIONS_CHANGED))}>{t("m5.device.refresh")}</button>}</div>}
    <div className="m5-connection-actions">
      {!guide && <button type="button" className="m5-connection-check" aria-label={t("m5.login.check")} disabled={checkDisabled} onClick={check}>
        <svg viewBox="0 0 24 24" aria-hidden="true" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round"><path d="M20 7v5h-5M20 12a8 8 0 1 0-2 5" /></svg>{t("m5.login.check")}</button>}
      {alwaysExpanded && <div className="m5-installation-slot" id={`${provider}-installation-slot`}/>}
    </div>
    {appConnected === false && !alwaysExpanded && <small>{t("m5.connection.retained")}</small>}
    {appConnected && canCollapse && <button type="button" aria-expanded={showDetails} aria-controls={`${provider}-connection-details`} onClick={()=>setExpanded(value=>!value)}>{t(showDetails ? "m5.login.hideDetails" : "m5.login.showDetails")}</button>}
    <div id={`${provider}-connection-details`} className="m5-connection-details" hidden={!showDetails || appConnected !== true}>
    {provider === "claude" && status.state === "waiting" && <form onSubmit={event=>{
      event.preventDefault();
      if (inFlight.current || !status.attemptId) return;
      const value=code.trim();
      if (!/^[a-zA-Z0-9_#.=~-]{1,4096}$/.test(value)) {setCodeError(true);return;}
      setCode("");setCodeError(false);
      void request({action:"submitCode",provider:"claude",attemptId:status.attemptId,code:value});
    }}>
      <label htmlFor="claude-connection-code">{t("m5.login.code.label")}</label>
      <p id="claude-code-help">{t("m5.login.code.help")}</p>
      <input id="claude-connection-code" type="password" autoComplete="off" spellCheck={false} maxLength={4096} aria-describedby="claude-code-help" value={code} onChange={event=>setCode(event.target.value)} disabled={disabled}/>
      <button type="submit" disabled={disabled||busy||!code.trim()}>{t("m5.login.code.submit")}</button>
      {codeError && <p role="alert">{t("m5.login.code.invalid")}</p>}
    </form>}
    {waiting ? (status.state === "waiting" || status.state === "verifying" ? <button className="m5-secondary" disabled={disabled||busy||!status.attemptId} onClick={()=>void request({action:"cancel",attemptId:status.attemptId!})}>{t("m5.login.cancel")}</button> : null)
      // A failure's guide offers its own way forward; the plain login button stays for a later retry.
      : status.state !== "connected" && !guide && <button className="m5-btn-primary" disabled={loginDisabled} aria-label={t("m5.login.startNamed", { name })} onClick={()=>void request({action:"start",attemptId:crypto.randomUUID()})}>{t("m5.login.start")}</button>}
    {guide && <RecoveryGuide tone={guide.tone} testId={`login-guide-${provider}`} className="m5-login-guide"
      title={t(`m5.login.guide.title.${guide.kind}`, { name })}
      what={guide.kind === "recovery" ? t("m5.login.recovery.help") : guide.kind === "unreachable" ? t("m5.login.error.unreachable") : loginErrorMessage(provider, error ? null : status.error, lang)}
      primary={guideAction(guide.primary)}
      alternatives={guide.alternatives.map(guideAction).filter((action): action is GuideAction => action !== null)}
      details={diagnostic ?? status.error ?? "providerLoginUnavailable"} />}
    <details className="m5-login-account-help">
      <summary>{t("m5.login.existing.title")}</summary>
      <p>{t("m5.login.existing.help", { name })}</p>
      <p>{t("m5.login.existing.saved")}</p>
    </details>
    </div>
  </section>;
}
