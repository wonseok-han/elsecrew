import { useEffect, useRef, useState } from "react";
import { FileSearchBusyError, type FileSearchResult } from "./file-search.ts";

export function FileSearch({ disabled, search, open }: {
  disabled: boolean;
  search: (query: string, signal: AbortSignal) => Promise<FileSearchResult>;
  open: (path: string) => void;
}) {
  const [query, setQuery] = useState("");
  const [result, setResult] = useState<FileSearchResult | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const active = useRef<AbortController | null>(null);
  const stop = () => { active.current?.abort(); active.current = null; setBusy(false); };
  useEffect(() => {
    if (disabled) { stop(); setResult(null); }
    return () => { active.current?.abort(); active.current = null; };
  }, [disabled]);
  return <section className="m5-file-search" aria-label="작업 복사본 파일 찾기">
    <form onSubmit={async event => {
      event.preventDefault(); if (disabled || !query.trim()) return;
      stop(); const request = new AbortController(); active.current = request;
      setBusy(true); setResult(null); setError(null);
      try {
        const found = await search(query, request.signal);
        if (active.current === request) setResult(found);
      } catch (cause) {
        if (active.current === request && !request.signal.aborted) setError(cause instanceof FileSearchBusyError ? "이전 검색을 마무리하고 있어요. 잠시 후 다시 찾아 주세요." : "파일 목록을 끝까지 읽지 못했어요. 다시 찾아 주세요.");
      } finally { if (active.current === request) { active.current = null; setBusy(false); } }
    }}>
      <input aria-label="파일 이름 또는 경로 찾기" placeholder="파일 이름 또는 경로" maxLength={256} value={query} disabled={disabled} onChange={event => { stop(); setQuery(event.target.value); setResult(null); setError(null); }} />
      <button type="submit" disabled={disabled || busy || !query.trim()}>찾기</button>
      {busy && <button type="button" onClick={stop}>중단</button>}
    </form>
    {busy && <p role="status">하위 폴더에서 파일 이름을 찾고 있어요…</p>}
    {error && <p role="alert">{error}</p>}
    {result && <>
      <p role="status">{result.matches.length}개{result.limited ? " · 검색 한도에 도달했어요. 경로를 더 구체적으로 입력해 주세요." : " · 하위 폴더까지 확인했어요."}</p>
      <div className="m5-search-results">{result.matches.map(entry => <button key={entry.path} type="button" disabled={disabled} title={entry.path} onClick={() => open(entry.path)}>{entry.path}</button>)}</div>
      <button type="button" onClick={() => setResult(null)}>검색 결과 닫기</button>
    </>}
  </section>;
}
