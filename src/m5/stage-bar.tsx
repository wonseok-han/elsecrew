import { useEffect, useState } from "react";
import { useI18n } from "../i18n.tsx";
import type { AgentRunRecordV1 } from "./contracts/agent-adapter-v1.ts";
import type { RoleDefinitionV3 } from "./contracts/run-recovery-v1.ts";
import { roleLabel } from "./role-names.ts";
import { isLeadRole, roleColorIndex } from "./role-colors.ts";
import { stageCells, stageNow, type StageCell } from "./task-story-model.ts";

/**
 * The stage bar (ADE 1차-나 L4a): one cell per run that actually happened,
 * the one working now outlined, and while the team works a last cell that
 * says what comes next — "the coordinator decides", or between runs "the
 * coordinator is reading the result and picking who goes next". It sticks
 * to the top of the story. What happens now is announced in one place.
 */
export function StageBar({ runs, working, definitions }: { runs: readonly AgentRunRecordV1[]; working: boolean; definitions?: readonly RoleDefinitionV3[] }) {
  const { t } = useI18n();
  const live = runs.some(run => ["preparing", "running", "cancelling"].includes(run.state));
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    if (!live) return;
    setNow(Date.now());
    const timer = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(timer);
  }, [live]);
  const { hidden, cells } = stageCells(runs, { working, now });
  if (cells.length === 0) return null;
  const duration = (seconds: number | null) => seconds === null ? "" : seconds >= 60 ? t("m5.stage.minutes", { n: Math.floor(seconds / 60) }) : t("m5.stage.seconds", { n: seconds });
  const title = (cell: StageCell) => cell.kind === "run" ? roleLabel(cell.roleId, definitions, t) : cell.kind === "next" ? t("m5.stage.next") : roleLabel("coordinator", definitions, t);
  const meta = (cell: StageCell) => {
    switch (cell.kind) {
      case "planning": return t("m5.stage.planning");
      case "between": return t("m5.stage.between");
      case "next": return t("m5.stage.nextHelp");
      case "run": {
        const time = duration(cell.seconds);
        const word = t(`m5.stage.status.${cell.status}`);
        return time ? `${word} · ${time}` : word;
      }
    }
  };
  const current = stageNow(cells);
  const announce = current ? `${title(current)} · ${current.kind === "run" ? t(`m5.stage.status.${current.status}`) : meta(current)}` : "";
  return <div className="m5-stage-bar" data-testid="stage-bar" role="list" aria-label={t("m5.stage.label")}>
    {hidden > 0 && <span className="m5-stage-more" role="listitem" title={t("m5.stage.hidden", { count: hidden })} aria-label={t("m5.stage.hidden", { count: hidden })}>+{hidden}</span>}
    {cells.map((cell, index) => {
      const status = cell.kind === "run" ? cell.status : cell.kind === "next" ? "next" : "now";
      const color = cell.kind === "run" ? roleColorIndex(cell.roleId) : cell.kind === "next" ? null : roleColorIndex("coordinator");
      return <div key={cell.kind === "run" ? cell.runId : `${cell.kind}-${index}`} role="listitem" className={`m5-stage m5-stage-${status}`} data-stage={cell.kind} aria-current={status === "now" ? "step" : undefined}>
        <span className="m5-stage-title">{color !== null && <span className="m5-role-dot" data-role-color={color} aria-hidden="true" />}<span className="m5-stage-name" title={title(cell)}><bdi>{title(cell)}</bdi></span>{cell.kind !== "next" && isLeadRole(cell.kind === "run" ? cell.roleId : "coordinator") && <span className="m5-role-lead" aria-label={t("m5.card.lead")}>★</span>}</span>
        <span className="m5-stage-meta">{meta(cell)}</span>
      </div>;
    })}
    <p className="m5-sr-only" aria-live="polite">{announce}</p>
  </div>;
}
