import { useCallback, useEffect, useRef, useState } from "react";
import { useI18n } from "../i18n";
import { loginErrorMessage } from "./provider-login-message.ts";
import { LOGIN_PROVIDERS, readProviderSelection, saveProviderSelection } from "./provider-selection.ts";
import { validateLoginStatus, type LoginRequest, type LoginStatus, type LoginProvider } from "./contracts/provider-login-v1.ts";
import { validateDeviceReport, type DeviceReport } from "./contracts/device-report-v1.ts";
import { ProviderInstall } from "./provider-install.tsx";
import { shouldPollLogin } from "./provider-login-poll.ts";

export function ProviderLogin({ disabled, compact = false }: {disabled: boolean; compact?: boolean}) {
  const { t } = useI18n();
  const [selected, setSelected] = useState(readProviderSelection);
  const [selectionExpanded, setSelectionExpanded] = useState(!compact && selected.length === 0);
  useEffect(() => { setSelectionExpanded(!compact && selected.length === 0); }, [compact, selected.length]);
  const [draft, setDraft] = useState<LoginProvider[]>([]);
  const [saveFailed, setSaveFailed] = useState(false);
  const [installBlocked, setInstallBlocked] = useState(Boolean(window.m5Install));
  const [checks, setChecks] = useState({ codex: 0, claude: 0 });
  const [step, setStep] = useState<"agents" | "install" | "login">("agents");
  const [loginAttention, setLoginAttention] = useState({ codex: false, claude: false });
  const reportLoginAttention = useCallback((provider: LoginProvider, attention: boolean) => {
    setLoginAttention(current => current[provider] === attention ? current : { ...current, [provider]: attention });
  }, []);
  const loginNeedsAttention = selected.some(provider => loginAttention[provider]);
  const navigate = (next: typeof step) => { if (!installBlocked) setStep(next); };
  const select = (providers: LoginProvider[]) => {
    if (disabled) return;
    setSaveFailed(!saveProviderSelection(providers));
    setSelected(providers);
    setSelectionExpanded(false);
    setStep("install");
  };
  return <>
    {!compact && <nav className="m5-setup-navigation" aria-label={t("m5.setup.steps.label")}>
      {(["agents", "install", "login"] as const).map((item, index) => <button type="button" key={item}
        aria-current={step === item ? "step" : undefined}
        disabled={installBlocked || (item !== "agents" && selected.length === 0)}
        onClick={() => navigate(item)}><span aria-hidden="true">{index + 1}</span>{t(`m5.setup.steps.${item}`)}</button>)}
    </nav>}
    <div hidden={!compact && step !== "agents"}>
    <DeviceCheck compact={compact}/>
    <details className="m5-provider-selection" open={selectionExpanded} onToggle={event => setSelectionExpanded(event.currentTarget.open)}>
      <summary><strong>{t("m5.setup.agents.title")}</strong><span>{selected.length ? selected.map(p => p === "codex" ? "Codex" : "Claude Code").join(" · ") : t("m5.setup.agents.choose")}</span></summary>
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
    </details>
    {!compact && selected.length > 0 && <button type="button" className="m5-setup-next" disabled={installBlocked} onClick={() => navigate("install")}>{t("m5.setup.steps.toInstall")}</button>}
    </div>
    {!selectionExpanded && saveFailed && <p className="m5-notice" role="status">{t("m5.setup.agents.saveFailed")}</p>}
    <div hidden={!compact && step !== "install" && !installBlocked}>
    <ProviderInstall disabled={disabled} providers={selected} compact={compact} onBlockChange={setInstallBlocked}
      onCheck={provider => { setChecks(current => ({ ...current, [provider]: current[provider] + 1 })); setStep("login"); }}/>
    {!compact && <div className="m5-setup-next"><p>{t("m5.setup.steps.installHelp")}</p><button type="button" disabled={installBlocked || selected.length === 0} onClick={() => navigate("login")}>{t("m5.setup.steps.toLogin")}</button></div>}
    </div>
    <div hidden={!compact && step !== "login" && !loginNeedsAttention}>
    {LOGIN_PROVIDERS.filter(provider => selected.includes(provider)).map(provider => <ProviderConnection key={`${provider}:${checks[provider]}`} disabled={disabled || installBlocked} provider={provider} onAttention={reportLoginAttention}/>)}
    {!compact && <p className="m5-setup-next">{t("m5.setup.steps.projectHelp")}</p>}
    </div>
  </>;
}
function DeviceCheck({ compact }: { compact: boolean }) {
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
  return <details className="m5-device-check" open={compact ? undefined : true}>
    <summary>{t("m5.device.title")}</summary>
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
  </details>;
}
function ProviderConnection({ disabled, provider, onAttention }: {disabled: boolean; provider: LoginProvider; onAttention: (provider: LoginProvider, attention: boolean) => void}) {
  const { lang, t } = useI18n();
  const name = provider === "codex" ? "Codex" : "Claude Code";
  const [status,setStatus] = useState<LoginStatus>({state:"idle",attemptId:null,authUrl:null,error:null});
  const [busy,setBusy] = useState(false);
  const [error,setError] = useState(false);
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
      if(mounted.current) { setStatus(result); setError(false); }
    } catch { if(mounted.current) setError(true); }
    finally { inFlight.current=false; if(mounted.current) setBusy(false); }
  };
  useEffect(() => {
    mounted.current=true;
    if(disabled) return;
    if (!checked.current) {
      checked.current=true;
      void request({action:"diagnose",attemptId:crypto.randomUUID()});
    } else void request({action:"status"});
    return () => { mounted.current=false; };
  },[disabled]);
  const poll = shouldPollLogin(status.state, error, disabled);
  useEffect(() => {
    if (!poll) return;
    const timer = window.setInterval(() => void request({action:"status"}), 500);
    return () => window.clearInterval(timer);
  }, [poll]);
  useEffect(() => {
    // Installation is not authentication. Only the existing account check can
    // report a connected account; diagnosis never reads credentials.
    if (!disabled && status.state === "installed") {
      void request({action:"check",attemptId:crypto.randomUUID()});
    }
  },[disabled,status.state]);
  useEffect(()=>{setCode("");setCodeError(false);},[status.attemptId,status.state]);
  useEffect(()=>{ if (status.state === "connected") setExpanded(false); },[status.state]);
  const waiting=status.state === "diagnosing" || status.state === "starting" || status.state === "waiting" || status.state === "verifying";
  const recoveryRequired=status.error === "providerLoginRecoveryRequired";
  useEffect(() => {
    onAttention(provider, waiting || recoveryRequired || error || status.state === "failed");
  }, [provider, waiting, recoveryRequired, error, status.state, onAttention]);
  const canCollapse=status.state === "connected" && !status.error && !error;
  const showDetails=!canCollapse || expanded;
  const label = t(`m5.login.state.${status.state}`, { name });
  return <section className="m5-provider-login" aria-label={t("m5.login.section", { name })}>
    <div><strong>{t("m5.login.section", { name })}</strong><p role="status">{label}</p><small>{t("m5.login.separate")}</small></div>
    {canCollapse && <button type="button" aria-expanded={showDetails} aria-controls={`${provider}-connection-details`} onClick={()=>setExpanded(value=>!value)}>{t(showDetails ? "m5.login.hideDetails" : "m5.login.showDetails")}</button>}
    <div id={`${provider}-connection-details`} hidden={!showDetails}>
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
      : <button disabled={disabled||busy||recoveryRequired||status.state==="connected"||status.state==="unsupported"||!window.m5Login} onClick={()=>void request({action:"start",attemptId:crypto.randomUUID()})}>{t(status.state==="connected"?"m5.login.connected":"m5.login.start", { name })}</button>}
    {!waiting && status.state!=="unsupported" && <button disabled={disabled||busy||recoveryRequired||!window.m5Login} onClick={()=>void request({action:"check",attemptId:crypto.randomUUID()})}>{t("m5.login.check")}</button>}
    {!waiting && status.state!=="unsupported" && <button disabled={disabled||busy||recoveryRequired||!window.m5Login} onClick={()=>void request({action:"diagnose",attemptId:crypto.randomUUID()})}>{t("m5.login.diagnose")}</button>}
    {recoveryRequired && <div><p>{t("m5.login.recovery.help")}</p><button disabled={disabled||busy||waiting||!window.m5Login} onClick={()=>void request({action:"recover",attemptId:crypto.randomUUID()})}>{t("m5.login.recovery.action")}</button></div>}
    {(error||status.state==="failed") && <div role="alert"><p>{loginErrorMessage(provider,status.error,lang)}</p><details><summary>{t("m5.login.detail")}</summary><code>{status.error ?? "providerLoginUnavailable"}</code></details></div>}
    </div>
  </section>;
}
