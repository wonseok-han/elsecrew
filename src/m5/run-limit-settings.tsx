import { useEffect, useState } from "react";
import { useI18n } from "../i18n.tsx";
import { RUN_LIMIT_MAX, validateProjectCatalogResult, type RunLimits } from "./contracts/project-catalog-v1.ts";
import { Setting, SettingsGroup } from "./settings-ui.tsx";

/**
 * How many tasks may use each agent at once (ADE multi-project P3′). More
 * wait their turn instead of stopping. Default 3 each (see the engine's
 * `app_run_limits.rs` for the memory it is based on).
 */
export function RunLimitSettings({ open }: { open: boolean }) {
  const { t } = useI18n();
  const [limits, setLimits] = useState<RunLimits | null>(null);
  const [failed, setFailed] = useState(false);
  useEffect(() => {
    if (!open || !window.m5Projects) return;
    let live = true;
    const input = { action: "getRunLimits" as const };
    void window.m5Projects.request(input).then(raw => {
      const result = validateProjectCatalogResult(raw, input);
      if (live && (result.action === "getRunLimits" || result.action === "setRunLimit")) setLimits(result.limits);
    }).catch(() => { if (live) setFailed(true); });
    return () => { live = false; };
  }, [open]);
  const choose = async (runtime: keyof RunLimits, limit: number) => {
    if (!window.m5Projects) return;
    setFailed(false);
    const input = { action: "setRunLimit" as const, runtime, limit };
    try {
      const result = validateProjectCatalogResult(await window.m5Projects.request(input), input);
      if (result.action === "getRunLimits" || result.action === "setRunLimit") setLimits(result.limits);
    } catch { setFailed(true); }
  };
  const choices = Array.from({ length: RUN_LIMIT_MAX }, (_, index) => index + 1);
  const row = (runtime: keyof RunLimits, name: string) => <Setting as="label" title={name}
    control={<select data-testid={`run-limit-${runtime}`} disabled={!limits} value={limits?.[runtime] ?? 3} onChange={event => void choose(runtime, Number(event.target.value))}>
      {choices.map(count => <option key={count} value={count}>{t("m5.runLimits.count", { count })}</option>)}</select>} />;
  return <>
    <SettingsGroup label={t("m5.runLimits.title")} testId="run-limits">
      {row("claude-code", "Claude Code")}
      {row("codex", "Codex")}
    </SettingsGroup>
    <p className="m5-settings-intro">{t("m5.runLimits.help")}</p>
    {failed && <p role="alert">{t("m5.runLimits.failed")}</p>}
  </>;
}
