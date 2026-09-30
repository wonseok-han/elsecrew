import { useEffect, useRef, useState } from "react";
import { useI18n } from "../i18n";
import { loginErrorMessage } from "./provider-login-message.ts";
import { validateLoginStatus, type LoginRequest, type LoginStatus, type LoginProvider } from "./contracts/provider-login-v1.ts";

export function ProviderLogin({ disabled }: {disabled: boolean}) {
  return <><ProviderConnection disabled={disabled} provider="codex"/><ProviderConnection disabled={disabled} provider="claude"/></>;
}
function ProviderConnection({ disabled, provider }: {disabled: boolean; provider: LoginProvider}) {
  const { lang, t } = useI18n();
  const name = provider === "codex" ? "Codex" : "Claude Code";
  const [status,setStatus] = useState<LoginStatus>({state:"idle",attemptId:null,authUrl:null,error:null});
  const [busy,setBusy] = useState(false);
  const [error,setError] = useState(false);
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
    const timer=window.setInterval(()=>void request({action:"status"}),500);
    return () => { mounted.current=false; window.clearInterval(timer); };
  },[disabled]);
  useEffect(() => {
    // Installation is not authentication. Only the existing account check can
    // report a connected account; diagnosis never reads credentials.
    if (!disabled && status.state === "installed") {
      void request({action:"check",attemptId:crypto.randomUUID()});
    }
  },[disabled,status.state]);
  useEffect(()=>{setCode("");setCodeError(false);},[status.attemptId,status.state]);
  const waiting=status.state === "diagnosing" || status.state === "starting" || status.state === "waiting" || status.state === "verifying";
  const recoveryRequired=status.error === "providerLoginRecoveryRequired";
  const label = t(`m5.login.state.${status.state}`, { name });
  return <section className="m5-provider-login" aria-label={t("m5.login.section", { name })}>
    <div><strong>{t("m5.login.section", { name })}</strong><p role="status">{label}</p><small>{t("m5.login.separate")}</small></div>
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
  </section>;
}
