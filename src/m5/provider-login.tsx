import { useEffect, useRef, useState } from "react";
import { validateLoginStatus, type LoginRequest, type LoginStatus } from "./contracts/provider-login-v1.ts";

export function ProviderLogin({ disabled }: {disabled: boolean}) {
  const [status,setStatus] = useState<LoginStatus>({state:"idle",attemptId:null,authUrl:null,error:null});
  const [busy,setBusy] = useState(false);
  const [error,setError] = useState(false);
  const inFlight = useRef(false);
  const mounted = useRef(true);
  const request = async (input: LoginRequest) => {
    if (inFlight.current || !window.m5Login) return;
    inFlight.current = true;
    setBusy(true);
    try {
      const result = validateLoginStatus(await window.m5Login.request(input));
      if(mounted.current) { setStatus(result); setError(false); }
    } catch { if(mounted.current) setError(true); }
    finally { inFlight.current=false; if(mounted.current) setBusy(false); }
  };
  useEffect(() => {
    mounted.current=true;
    if(disabled) return;
    void request({action:"status"});
    const timer=window.setInterval(()=>void request({action:"status"}),500);
    return () => { mounted.current=false; window.clearInterval(timer); };
  },[disabled]);
  const waiting=status.state === "starting" || status.state === "waiting";
  const label = {idle:"아직 연결하지 않았어요",starting:"로그인을 준비하고 있어요…",waiting:"브라우저에서 로그인을 완료해 주세요",connected:"이 앱에 Codex 로그인이 연결됐어요",canceled:"로그인을 취소했어요",failed:"로그인을 완료하지 못했어요",unsupported:"이 환경에서는 아직 연결할 수 없어요"}[status.state];
  return <section className="m5-provider-login" aria-label="에이전트 연결">
    <div><strong>Codex 연결</strong><p role="status">{label}</p><small>기존 터미널 로그인과 별도로 연결해요. 모델을 실행하거나 비용을 사용하지 않아요.</small></div>
    {waiting ? <button disabled={disabled||busy||!status.attemptId} onClick={()=>void request({action:"cancel",attemptId:status.attemptId!})}>로그인 취소</button>
      : <button disabled={disabled||busy||status.state==="connected"||status.state==="unsupported"||!window.m5Login} onClick={()=>void request({action:"start",attemptId:crypto.randomUUID()})}>{status.state==="connected"?"연결됨":"Codex 로그인"}</button>}
    {(error||status.state==="failed") && <div role="alert"><p>{status.error === "providerLoginRecoveryRequired" ? "로그인 프로세스 종료를 확인하지 못했어요. 새 로그인과 안전 종료를 차단하고 복구 기록을 보존했어요. 인증 파일을 지우거나 강제 종료하지 말고 아래 정보로 도움을 요청해 주세요." : "현재 지원하는 Codex 0.153.4가 설치되어 있는지 확인해 주세요. 기존 터미널 인증 파일은 수정하거나 삭제하지 마세요."}</p><details><summary>도움 요청용 정보</summary><code>{status.error ?? "providerLoginUnavailable"}</code></details></div>}
  </section>;
}
