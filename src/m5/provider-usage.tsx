import { useEffect, useRef, useState } from "react";
import { useI18n } from "../i18n.tsx";
import { loadModels, catalogCheckedAt } from "./model-catalog.ts";
import type { ModelChoicesResult } from "./contracts/model-choices-v1.ts";

type Entry = { usage: ModelChoicesResult["usage"]; checkedAt: number; failed: boolean };
// No durable account usage; each app session starts without a claimed balance.
export function ProviderUsage({ disabled }: { disabled: boolean }) {
  const { t, lang } = useI18n();
  const [entries, setEntries] = useState<Partial<Record<"codex" | "claude-code", Entry>>>({});
  const [busy, setBusy] = useState(false);
  const pending = useRef(false);
  const mounted = useRef(false);
  const refresh = async (force = false) => {
    if (disabled || !window.m5Models || pending.current) return;
    pending.current = true; setBusy(true);
    for (const runtime of ["codex"] as const) {
      if (!mounted.current) break;
      try {
        const result = await loadModels(runtime, input => window.m5Models!.request(input), force);
        if (mounted.current) setEntries(previous => ({ ...previous, [runtime]: { usage: result.usage, checkedAt: catalogCheckedAt(runtime) ?? Date.now(), failed: false } }));
      } catch {
        // A failed refresh cannot claim that the previously observed balance is current.
        if (mounted.current) setEntries(previous => ({ ...previous, [runtime]: { usage: null, checkedAt: Date.now(), failed: true } }));
      }
    }
    pending.current = false; if (mounted.current) setBusy(false);
  };
  useEffect(() => { mounted.current = true; return () => { mounted.current = false; }; }, []);
  useEffect(() => { if (!disabled) void refresh(); }, [disabled]);
  return <details className="m5-provider-usage" onToggle={event => { if (event.currentTarget.open) void refresh(); }}>
    <summary title={t("m5.usage.title")}><span>{t("m5.usage.title")}</span><span>{entries.codex?.usage?.windows[0] ? `Codex ${Math.round(entries.codex.usage.windows[0].usedPercent)}%` : "—"}</span></summary>
    <div className="m5-provider-usage-content">
      {(["codex", "claude-code"] as const).map(runtime => {
        const entry = entries[runtime];
        return <section key={runtime}><strong>{runtime === "codex" ? "Codex" : "Claude Code"}</strong>
          {entry?.usage?.windows.length ? entry.usage.windows.map(window => <div key={window.id}>
            <div className="m5-usage-window"><span>{window.windowMinutes ? (window.windowMinutes >= 1440 ? t("m5.usage.days", { n: window.windowMinutes / 1440 }) : t("m5.usage.hours", { n: window.windowMinutes / 60 })) : t("m5.usage.window")}</span><span>{t("m5.usage.used", { n: Math.round(window.usedPercent) })}</span></div>
            <progress max={100} value={window.usedPercent} aria-label={t("m5.usage.used", { n: Math.round(window.usedPercent) })}/>
            {window.resetsAt !== null && <small>{t("m5.usage.reset", { time: new Date(window.resetsAt * 1000).toLocaleString(lang) })}</small>}
          </div>) : <small>{t(busy && !entry ? "m5.usage.loading" : runtime === "claude-code" && !entry?.failed ? "m5.usage.notProvided" : "m5.usage.unavailable")}</small>}
          {entry?.usage && <small>{t("m5.usage.checked", { time: new Date(entry.checkedAt).toLocaleTimeString(lang) })}</small>}
        </section>;
      })}
      <button type="button" disabled={disabled || busy} onClick={() => void refresh(true)}>{t(busy ? "m5.usage.loading" : "m5.usage.refresh")}</button>
    </div>
  </details>;
}
