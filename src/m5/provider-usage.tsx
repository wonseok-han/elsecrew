import { readConnections, CONNECTIONS_CHANGED, PROVIDER_AUTH_CHANGED, type Connections } from "./provider-connections.ts";
import type { ClaudeUsageResult } from "./contracts/provider-usage-v1.ts";
import { useEffect, useId, useRef, useState } from "react";
import { useI18n } from "../i18n.tsx";
import { loadModels, catalogCheckedAt } from "./model-catalog.ts";
import type { ModelChoicesResult } from "./contracts/model-choices-v1.ts";

type Entry = { usage: ModelChoicesResult["usage"]; checkedAt: number; failed: boolean; state?: ClaudeUsageResult["state"] };
// No durable account usage; each app session starts without a claimed balance.
export function ProviderUsage({ disabled }: { disabled: boolean }) {
  const { t, lang } = useI18n();
  const [entries, setEntries] = useState<Partial<Record<"codex" | "claude-code", Entry>>>({});
  const [connections,setConnections] = useState<Connections>({codex:false,"claude-code":false});
  const generation = useRef(0);
  const [connectionRevision,setConnectionRevision] = useState(0);
  const [busy, setBusy] = useState(false);
  const popupId = useId();
  const trigger = useRef<HTMLButtonElement>(null);
  const popup = useRef<HTMLDivElement>(null);
  const pending = useRef(false);
  const mounted = useRef(false);
  const refresh = async (force = false) => {
    if (disabled || pending.current) return;
    const epoch = generation.current;
    pending.current = true; setBusy(true);
    try {
      const connected=await readConnections();
      if (!mounted.current || epoch !== generation.current) return;
      setConnections(connected);
      await Promise.all((["codex","claude-code"] as const).map(async runtime => {
        let entry: Entry = {usage:null,checkedAt:Date.now(),failed:false,state:"notConnected"};
        try {
          if (connected[runtime]) {
            if(runtime === "codex" && window.m5Models) {
              const result=await loadModels(runtime,input=>window.m5Models!.request(input),force);
              entry={usage:result.usage,checkedAt:catalogCheckedAt(runtime) ?? Date.now(),failed:false};
            } else if(runtime === "claude-code" && window.m5ClaudeUsage) {
              const result=await window.m5ClaudeUsage.read();
              entry={...result,failed:result.state!=="available"};
            } else entry={usage:null,checkedAt:Date.now(),failed:true};
          }
        } catch { entry={usage:null,checkedAt:Date.now(),failed:true}; }
        if(mounted.current && epoch===generation.current) setEntries(previous=>({...previous,[runtime]:entry}));
      }));
    } catch { if(mounted.current) setEntries({}); }
    finally { pending.current=false; if(mounted.current) { setBusy(false); if(epoch !== generation.current) setConnectionRevision(value=>value+1); } }
  };
  useEffect(() => { mounted.current=true; return()=>{mounted.current=false;generation.current++;}; },[]);
  useEffect(() => { if(!disabled) void refresh(true); },[disabled,connectionRevision]);
  useEffect(() => {
    const changed=()=>{generation.current++;setEntries({});setConnectionRevision(value=>value+1);void readConnections().then(value=>{if(mounted.current)setConnections(value);}).catch(()=>{});};
    window.addEventListener(CONNECTIONS_CHANGED,changed);
    window.addEventListener(PROVIDER_AUTH_CHANGED,changed);
    return()=>{window.removeEventListener(CONNECTIONS_CHANGED,changed);window.removeEventListener(PROVIDER_AUTH_CHANGED,changed);};
  },[]);
  const positionPopup = () => {
    const anchor = trigger.current?.getBoundingClientRect();
    const panel = popup.current;
    if (!anchor || !panel) return;
    panel.style.left = `${Math.max(8, Math.min(anchor.left, window.innerWidth - panel.offsetWidth - 8))}px`;
    panel.style.top = `${Math.max(8, Math.min(anchor.top - panel.offsetHeight - 8, window.innerHeight - panel.offsetHeight - 8))}px`;
  };
  useEffect(() => { if (popup.current?.matches(":popover-open")) positionPopup(); }, [entries, busy]);
  useEffect(() => {
    window.addEventListener("resize", positionPopup);
    return () => window.removeEventListener("resize", positionPopup);
  }, []);
  return <div className="m5-provider-usage">
    <button ref={trigger} type="button" className="m5-usage-trigger" popoverTarget={popupId} title={t("m5.usage.title")} aria-label={t("m5.usage.title")}>
      {(["codex","claude-code"] as const).map(runtime => {
        const entry=entries[runtime];
        return <span className="m5-usage-summary" key={runtime}><b>{runtime === "codex" ? "Codex" : "Claude"}</b>{!connections[runtime] ? <span>{t("m5.connection.off")}</span> : entry?.usage?.windows.length ? entry.usage.windows.map(item=><span key={item.id}>{item.windowMinutes ? (item.windowMinutes >= 1440 ? t("m5.usage.days",{n:item.windowMinutes/1440}) : t("m5.usage.hours",{n:item.windowMinutes/60})) : t("m5.usage.window")} {Math.round(item.usedPercent)}%</span>) : <span>{busy ? t("m5.usage.loading") : "—"}</span>}</span>;
      })}
    </button>
    <div ref={popup} id={popupId} popover="auto" className="m5-provider-usage-content" onToggle={event => { if (event.newState === "open") { positionPopup(); void refresh(); } }}>
      <header><strong>{t("m5.usage.title")}</strong><button className="m5-usage-refresh" type="button" aria-label={t("m5.usage.refresh")} title={t("m5.usage.refresh")} aria-busy={busy} disabled={disabled || busy} onClick={() => void refresh(true)}><svg viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><path d="M20 7v5h-5M20 12a8 8 0 1 0-2 5M20 7l-3-3"/></svg></button></header>
      {(["codex", "claude-code"] as const).map(runtime => {
        const entry = entries[runtime];
        return <section key={runtime}><strong>{runtime === "codex" ? "Codex" : "Claude Code"}</strong>
          {entry?.usage?.windows.length ? entry.usage.windows.map(window => <div key={window.id}>
            <div className="m5-usage-window"><span>{window.windowMinutes ? (window.windowMinutes >= 1440 ? t("m5.usage.days", { n: window.windowMinutes / 1440 }) : t("m5.usage.hours", { n: window.windowMinutes / 60 })) : t("m5.usage.window")}</span><span>{t("m5.usage.used", { n: Math.round(window.usedPercent) })}</span></div>
            <progress max={100} value={window.usedPercent} aria-label={t("m5.usage.used", { n: Math.round(window.usedPercent) })}/>
            {window.resetsAt !== null && <small>{t("m5.usage.reset", { time: new Date(window.resetsAt * 1000).toLocaleString(lang) })}</small>}
          </div>) : <small>{t(busy && !entry ? "m5.usage.loading" : entry?.state === "expired" ? "m5.usage.expired" : entry?.state === "notConnected" ? (connections[runtime] ? "m5.usage.notConnected" : "m5.connection.off") : entry?.state === "retryLater" ? "m5.usage.retryLater" : "m5.usage.unavailable")}</small>}
          {entry?.usage && <small>{t("m5.usage.checked", { time: new Date(entry.checkedAt).toLocaleTimeString(lang) })}</small>}
        </section>;
      })}
    </div>
  </div>;
}
