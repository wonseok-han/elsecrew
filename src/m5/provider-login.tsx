import { readConnections, changeConnection, CONNECTIONS_CHANGED } from "./provider-connections.ts";
import { useCallback, useEffect, useRef, useState } from "react";
import { useI18n } from "../i18n";
import { loginErrorMessage } from "./provider-login-message.ts";
import { LOGIN_PROVIDERS, readProviderSelection, saveProviderSelection } from "./provider-selection.ts";
import { validateLoginStatus, type LoginRequest, type LoginStatus, type LoginProvider } from "./contracts/provider-login-v1.ts";
import { validateDeviceReport, type DeviceReport } from "./contracts/device-report-v1.ts";
import { ProviderInstall } from "./provider-install.tsx";
import { shouldPollLogin } from "./provider-login-poll.ts";
import { readLoginDiagnostic, type LoginDiagnostic } from "./provider-login-diagnostic.ts";

export type ConnectionReveal = { provider: LoginProvider; sequence: number };

export function ProviderLogin({ disabled, compact: preferCompact = false, returning = false, settingsPage = false, reveal }: {disabled: boolean; compact?: boolean; returning?: boolean; settingsPage?: boolean; reveal?: ConnectionReveal}) {
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
    <DeviceCheck expanded={settingsPage}/>
    <SelectionContainer className="m5-provider-selection" open={selectionExpanded} onToggle={event => setSelectionExpanded(event.currentTarget.open)}>
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
    </SelectionContainer>
    {!settingsPage && !compact && selected.length > 0 && <button type="button" className="m5-setup-next" disabled={installBlocked} onClick={() => navigate("install")}>{t("m5.setup.steps.toInstall")}</button>}
    </div>
    {!selectionExpanded && saveFailed && <p className="m5-notice" role="status">{t("m5.setup.agents.saveFailed")}</p>}
    <div hidden={!settingsPage && !compact && step !== "install" && !installBlocked}>
    <ProviderInstall slotsRevision={`${checks.codex}:${checks.claude}`} integrated={settingsPage} disabled={disabled} providers={settingsPage ? [...LOGIN_PROVIDERS] : selected} compact={compact} onBlockChange={setInstallBlocked} onAttention={setInstallAttention}
      onCheck={provider => { setChecks(current => ({ ...current, [provider]: current[provider] + 1 })); setStep("login"); }}/>
    {!settingsPage && !compact && <div className="m5-setup-next"><p>{t("m5.setup.steps.installHelp")}</p><button type="button" disabled={installBlocked || selected.length === 0} onClick={() => navigate("login")}>{t("m5.setup.steps.toLogin")}</button></div>}
    </div>
    <div hidden={!settingsPage && !compact && step !== "login" && !loginNeedsAttention}>
    {LOGIN_PROVIDERS.filter(provider => settingsPage || selected.includes(provider)).map(provider => <ProviderConnection key={`${provider}:${checks[provider]}`} disabled={disabled || installBlocked} provider={provider} alwaysExpanded={settingsPage} onAttention={reportLoginAttention}/>)}
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
function ProviderConnection({ disabled, provider, alwaysExpanded = false, onAttention }: {alwaysExpanded?: boolean; disabled: boolean; provider: LoginProvider; onAttention: (provider: LoginProvider, attention: boolean) => void}) {
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
    try { const value=await changeConnection(runtime,!appConnected); setAppConnected(value[runtime]); }
    catch { setConnectionError(true); }
    finally {setConnectionBusy(false);}
  };
  const [status,setStatus] = useState<LoginStatus>({state:"idle",attemptId:null,authUrl:null,error:null});
  const [busy,setBusy] = useState(false);
  const [error,setError] = useState(false);
  const [diagnostic,setDiagnostic] = useState<LoginDiagnostic | null>(null);
  const [expanded,setExpanded] = useState(false);
  const inFlight = useRef(false);
  const mounted = useRef(true);
  const checked = useRef(false);
  const [code,setCode] = useState("");
  const [codeError,setCodeError] = useState(false);
  const request = async (input: LoginRequest) => {
    if (inFlight.current || !window.m5Login) return;
    inFlight.current = true;
    setBusy(true);
    try {
      const scoped: LoginRequest = input.action === "submitCode" ? input : {...input,provider};
      const result = validateLoginStatus(await window.m5Login.request(scoped),provider);
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
  useEffect(()=>{setCode("");setCodeError(false);},[status.attemptId,status.state]);
  useEffect(()=>{ if (status.state === "connected") setExpanded(false); },[status.state]);
  const waiting=status.state === "diagnosing" || status.state === "starting" || status.state === "waiting" || status.state === "verifying";
  const recoveryRequired=status.error === "providerLoginRecoveryRequired";
  const profileCompatibilityRequired=status.error === "providerProfileIsolationRequired";
  useEffect(() => {
    onAttention(provider, waiting || recoveryRequired || error || status.state === "failed");
  }, [provider, waiting, recoveryRequired, error, status.state, onAttention]);
  const canCollapse=!alwaysExpanded && status.state === "connected" && !status.error && !error;
  const showDetails=!canCollapse || expanded;
  const label = appConnected === true ? t(`m5.login.state.${status.state}`, { name }) : t("m5.connection.off");
  return <section className="m5-provider-login" aria-label={t("m5.login.section", { name })}>
    <div className="m5-connection-heading"><strong>{t("m5.login.section", { name })}</strong><p role="status">{label}</p>{["installed","connected"].includes(status.state) && <small>{t("m5.install.detected")}</small>}</div>
    <button className="m5-connection-toggle" type="button" disabled={disabled || connectionBusy || appConnected === null || waiting} onClick={()=>void toggleConnection()}>{t(appConnected ? "m5.connection.disconnect" : "m5.connection.connect")}</button>
    <small className="m5-connection-help">{t("m5.login.separate")}</small>
    {connectionError && <div className="m5-connection-error" role="alert"><span>{t(appConnected === null ? "m5.connection.loadFailed" : "m5.connection.failed")}</span>{appConnected === null && <button type="button" aria-label={t("m5.device.refresh")} title={t("m5.device.refresh")} onClick={() => window.dispatchEvent(new Event(CONNECTIONS_CHANGED))}>↻</button>}</div>}
    {alwaysExpanded && <div className="m5-installation-slot" id={`${provider}-installation-slot`}/>}
    {appConnected === false && <small>{t("m5.connection.retained")}</small>}
    {appConnected && canCollapse && <button type="button" aria-expanded={showDetails} aria-controls={`${provider}-connection-details`} onClick={()=>setExpanded(value=>!value)}>{t(showDetails ? "m5.login.hideDetails" : "m5.login.showDetails")}</button>}
    <div id={`${provider}-connection-details`} hidden={!showDetails || appConnected !== true}>
    <details className="m5-login-account-help">
      <summary>{t("m5.login.existing.title")}</summary>
      <p>{t("m5.login.existing.help", { name })}</p>
      <p>{t("m5.login.existing.saved")}</p>
    </details>
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
    {waiting ? <button disabled={disabled||busy||!status.attemptId} onClick={()=>void request({action:"cancel",attemptId:status.attemptId!})}>{t("m5.login.cancel")}</button>
      : <button disabled={disabled||busy||recoveryRequired||profileCompatibilityRequired||status.state==="connected"||status.state==="unsupported"||!window.m5Login} onClick={()=>void request({action:"start",attemptId:crypto.randomUUID()})}>{t(status.state==="connected"?"m5.login.connected":"m5.login.start", { name })}</button>}
    {!waiting && status.state!=="unsupported" && <button disabled={disabled||busy||recoveryRequired||!window.m5Login} onClick={()=>void request({action:"check",attemptId:crypto.randomUUID()})}>{t("m5.login.check")}</button>}
    {!waiting && status.state!=="unsupported" && <button disabled={disabled||busy||recoveryRequired||!window.m5Login} onClick={()=>void request({action:"diagnose",attemptId:crypto.randomUUID()})}>{t("m5.login.diagnose")}</button>}
    {recoveryRequired && <div><p>{t("m5.login.recovery.help")}</p><button disabled={disabled||busy||waiting||!window.m5Login} onClick={()=>void request({action:"recover",attemptId:crypto.randomUUID()})}>{t("m5.login.recovery.action")}</button></div>}
    {(error||status.state==="failed") && <div role="alert"><p>{loginErrorMessage(provider,status.error,lang)}</p><details><summary>{t("m5.login.detail")}</summary><code>{diagnostic ?? status.error ?? "providerLoginUnavailable"}</code></details></div>}
    </div>
  </section>;
}
