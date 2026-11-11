import { useEffect, useRef, useState } from "react";
import { useI18n } from "../i18n.tsx";
import { FileSearchBusyError, type FileSearchResult } from "./file-search.ts";

export function FileSearch({ disabled, search, open, showHidden = false }: {
  disabled: boolean; showHidden?: boolean;
  search: (query: string, signal: AbortSignal) => Promise<FileSearchResult>;
  open: (path: string) => void;
}) {
  const { t } = useI18n();
  const [query, setQuery] = useState("");
  const [result, setResult] = useState<FileSearchResult | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<"previousBusy" | "failed" | null>(null);
  const active = useRef<AbortController | null>(null);
  const stop = () => { active.current?.abort(); active.current = null; setBusy(false); };
  useEffect(() => {
    if (disabled) { stop(); setResult(null); }
    return () => { active.current?.abort(); active.current = null; };
  }, [disabled]);
  const matches = result?.matches.filter(entry => showHidden || !entry.path.split("/").some(part => part.startsWith("."))) ?? [];
  return <section className="m5-file-search" aria-label={t("m5.search.title")}>
    <form onSubmit={async event => {
      event.preventDefault(); if (disabled || !query.trim()) return;
      stop(); const request = new AbortController(); active.current = request;
      setBusy(true); setResult(null); setError(null);
      try {
        const found = await search(query, request.signal);
        if (active.current === request) setResult(found);
      } catch (cause) {
        if (active.current === request && !request.signal.aborted) setError(cause instanceof FileSearchBusyError ? "previousBusy" : "failed");
      } finally { if (active.current === request) { active.current = null; setBusy(false); } }
    }}>
      <input aria-label={t("m5.search.input")} placeholder={t("m5.search.placeholder")} maxLength={256} value={query} disabled={disabled} onChange={event => { stop(); setQuery(event.target.value); setResult(null); setError(null); }} />
      <button type="submit" disabled={disabled || busy || !query.trim()}>{t("m5.search.find")}</button>
      {busy && <button type="button" onClick={stop}>{t("m5.search.stop")}</button>}
    </form>
    {busy && <p role="status">{t("m5.search.busy")}</p>}
    {error && <p role="alert">{t(`m5.search.${error}`)}</p>}
    {result && <>
      <p role="status">{t("m5.search.count", { count: matches.length })}{result.limited ? t("m5.search.limited") : t("m5.search.complete")}</p>
      <div className="m5-search-results">{matches.map(entry => <button key={entry.path} type="button" disabled={disabled} title={entry.path} onClick={() => open(entry.path)}>{entry.path}</button>)}</div>
      <button type="button" onClick={() => setResult(null)}>{t("m5.search.close")}</button>
    </>}
  </section>;
}
