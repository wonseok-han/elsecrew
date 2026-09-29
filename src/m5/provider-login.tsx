import { useEffect, useRef, useState } from "react";
import { loginErrorMessage } from "./provider-login-message.ts";
import { validateLoginStatus, type LoginRequest, type LoginStatus, type LoginProvider } from "./contracts/provider-login-v1.ts";

export function ProviderLogin({ disabled }: {disabled: boolean}) {
  return <><ProviderConnection disabled={disabled} provider="codex"/><ProviderConnection disabled={disabled} provider="claude"/></>;
}
function ProviderConnection({ disabled, provider }: {disabled: boolean; provider: LoginProvider}) {
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
      void request({action:"check",attemptId:crypto.randomUUID()});
    } else void request({action:"status"});
    const timer=window.setInterval(()=>void request({action:"status"}),500);
    return () => { mounted.current=false; window.clearInterval(timer); };
  },[disabled]);
  useEffect(()=>{setCode("");setCodeError(false);},[status.attemptId,status.state]);
  const waiting=status.state === "starting" || status.state === "waiting" || status.state === "verifying";
  const label = {idle:"아직 연결하지 않았어요",starting:"연결 상태를 확인하고 있어요…",waiting:"브라우저에서 로그인을 완료해 주세요",verifying:"연결 코드를 확인하고 있어요…",connected:`이 앱에 ${name} 로그인이 연결됐어요`,canceled:"로그인을 취소했어요",failed:"연결을 확인하지 못했어요",unsupported:"이 환경에서는 아직 연결할 수 없어요"}[status.state];
  return <section className="m5-provider-login" aria-label={`${name} 연결`}>
    <div><strong>{name} 연결</strong><p role="status">{label}</p><small>기존 터미널 로그인과 별도로 연결해요. 모델을 실행하거나 비용을 사용하지 않아요.</small></div>
    {provider === "claude" && status.state === "waiting" && <form onSubmit={event=>{
      event.preventDefault();
      if (inFlight.current || !status.attemptId) return;
      const value=code.trim();
      if (!/^[a-zA-Z0-9_#.=~-]{1,4096}$/.test(value)) {setCodeError(true);return;}
      setCode("");setCodeError(false);
      void request({action:"submitCode",provider:"claude",attemptId:status.attemptId,code:value});
    }}>
      <label htmlFor="claude-connection-code">연결 코드 붙여넣기</label>
      <p id="claude-code-help">브라우저의 마지막 화면에 표시된 코드 전체를 붙여넣어 주세요. 이메일 확인 번호가 아니에요.</p>
      <input id="claude-connection-code" type="password" autoComplete="off" spellCheck={false} maxLength={4096} aria-describedby="claude-code-help" value={code} onChange={event=>setCode(event.target.value)} disabled={disabled}/>
      <button type="submit" disabled={disabled||busy||!code.trim()}>연결 확인</button>
      {codeError && <p role="alert">코드 전체를 다시 복사해 주세요. 주소가 아닌 연결 코드를 입력해야 해요.</p>}
    </form>}
    {waiting ? <button disabled={disabled||busy||!status.attemptId} onClick={()=>void request({action:"cancel",attemptId:status.attemptId!})}>로그인 취소</button>
      : <button disabled={disabled||busy||status.state==="connected"||status.state==="unsupported"||!window.m5Login} onClick={()=>void request({action:"start",attemptId:crypto.randomUUID()})}>{status.state==="connected"?"연결됨":`${name} 로그인`}</button>}
    {status.error === "providerLoginRecoveryRequired" && <div><p>종료가 확인된 기록만 복구해요. 로그인 정보는 지우지 않으며, 증거가 부족하면 잠금을 유지해요.</p><button disabled={disabled||busy||waiting||!window.m5Login} onClick={()=>void request({action:"recover",attemptId:crypto.randomUUID()})}>연결 잠금 복구</button></div>}
    {(error||status.state==="failed") && <div role="alert"><p>{loginErrorMessage(provider,status.error)}</p><details><summary>도움 요청용 정보</summary><code>{status.error ?? "providerLoginUnavailable"}</code></details></div>}
  </section>;
}
