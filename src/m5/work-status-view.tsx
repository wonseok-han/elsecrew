import { useEffect, useRef, useState } from "react";
import type { SupportedM5Bridge } from "./bridge-boundary.ts";
import type { M5BridgeV6 } from "./contracts/bridge-v6.ts";
import { summarizeWorkStatus, type WorkStatusSummary } from "./work-status.ts";

/** Read-only status: no automatic cancellation, lock release, or recovery mutation. */
export function WorkStatusNotice({ bridge, ready, refreshKey }: {
  bridge: SupportedM5Bridge; ready: boolean; refreshKey: string;
}) {
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
        setSummary(summarizeWorkStatus(status)); setFailed(false);
      } catch { if (current) setFailed(true); }
      finally { if (current) setBusy(false); }
    })();
    inFlight.current = request;
    return () => { current = false; };
  }, [bridge, ready, refreshKey, refresh]);
  if (!ready || bridge.version < 6 || (!summary && !failed)) return null;
  return <section className="m5-admission" aria-label="진행 및 복구 상태" aria-busy={busy}>
    <strong>{failed ? "작업 상태를 다시 확인해야 해요." : "진행 중이거나 종료 확인이 필요한 작업이 있어요."}</strong>
    <p>{failed ? "상태를 불러오지 못했어요. 이전 표시만으로 종료됐다고 판단하지 마세요." : "진행 중인 작업은 끝날 때까지 기다려 주세요. 작업이 끝났는데도 계속 표시되면 해당 채팅의 복구 안내를 확인해 주세요. 채팅을 만들기 전에 실패했다면 저장된 프로젝트의 시작 실패 확인을 사용하세요. 복구가 거절되면 아래 확인 정보를 함께 전달해 주세요."}</p>
    <p>이 화면은 상태만 확인하며, 잠금이나 작업 파일을 삭제하지 않아요. 앱을 다시 열거나 재부팅하면 해결된다고 단정할 수 없어요.</p>
    {summary && <details><summary>도움 요청용 확인 정보 · {summary.total}건{failed ? " · 이전 조회" : ""}</summary>
      <ul>{summary.ids.map((id, index) => <li key={index}><code>{id}</code></li>)}</ul>
      {summary.omitted > 0 && <p>그 외 {summary.omitted}건</p>}
    </details>}
    <button type="button" className="m5-secondary" disabled={busy} onClick={() => setRefresh(value => value + 1)}>{busy ? "확인 중…" : "상태 다시 확인"}</button>
  </section>;
}
