import { useEffect, useRef, useState } from "react";
import { validateProjectCatalogResult, type PreparationInspection, type ProjectCatalogRequest } from "./contracts/project-catalog-v1.ts";

export function PreparationRecovery({ projectId, disabled }: { projectId: string; disabled: boolean }) {
  const [inspection, setInspection] = useState<PreparationInspection | null>(null);
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState("");
  const epoch = useRef(0);
  const pending = useRef(false);
  useEffect(() => {
    epoch.current++; setInspection(null); setMessage("");
    return () => { epoch.current++; };
  }, [projectId, disabled]);
  const request = async (input: ProjectCatalogRequest) => {
    if (disabled || pending.current || !window.m5Projects) return;
    const generation = epoch.current;
    pending.current = true; setBusy(true); setMessage("");
    try {
      const result = validateProjectCatalogResult(await window.m5Projects.request(input), input);
      if (epoch.current !== generation) return;
      if (result.action === "inspectPreparation") setInspection(result.inspection);
      else if (result.action === "abandonPreparation") {
        setInspection(null);
        setMessage("실패한 준비 기록을 종료했어요. 위 프로젝트 버튼으로 다시 열어 새 작업을 시작하세요. 기존 파일은 삭제하지 않았어요.");
      }
    } catch {
      if (epoch.current === generation) setMessage(input.action === "abandonPreparation"
        ? "결과를 확인하지 못했어요. 같은 준비 기록의 종료를 다시 확인할 수 있어요. 상태가 바뀌었다면 거절됩니다."
        : "안전하게 종료할 수 있는 미시작 준비를 확인하지 못했어요. 실행 중이거나 이전 형식인 기록은 여기서 해제하지 않아요.");
    } finally { pending.current = false; setBusy(false); }
  };
  return <div aria-label="프로젝트 시작 실패 복구" aria-busy={busy}>
    <button type="button" disabled={disabled || busy} onClick={() => { setInspection(null); void request({ action: "inspectPreparation", projectId }); }}>시작 실패 확인</button>
    {inspection && <div role="group" aria-label="실패한 준비 종료 확인">
      <p>실행 시작 전 실패한 준비 기록이에요. 이 준비를 종료하고 새 작업을 만들까요? 원본과 작업 파일은 삭제하지 않아요.</p>
      <code>{inspection.worktreeId}</code>
      <button type="button" disabled={disabled || busy} onClick={() => void request({ action: "abandonPreparation", inspection })}>이 준비 기록 종료</button>
      <button type="button" disabled={busy} onClick={() => setInspection(null)}>돌아가기</button>
    </div>}
    {message && <p role="status">{message}</p>}
  </div>;
}
