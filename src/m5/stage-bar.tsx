import { useEffect, useLayoutEffect, useRef, useState } from "react";
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
 *
 * Folded, it shows the last few stages and "+N" for the earlier ones. "+N"
 * opens every stage of the task in one row that scrolls sideways, ending on
 * the newest; a finished stage then takes the story to that member's card.
 */
export function StageBar({ runs, working, definitions }: { runs: readonly AgentRunRecordV1[]; working: boolean; definitions?: readonly RoleDefinitionV3[] }) {
  const { t } = useI18n();
  const [expanded, setExpanded] = useState(false);
  const rowRef = useRef<HTMLDivElement>(null);
  const chatId = runs[0]?.chatId ?? null;
  // Another task starts folded.
  useEffect(() => { setExpanded(false); }, [chatId]);
  const live = runs.some(run => ["preparing", "running", "cancelling"].includes(run.state));
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    if (!live) return;
    setNow(Date.now());
    const timer = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(timer);
  }, [live]);
  const { hidden, cells } = stageCells(runs, { working, now, max: expanded ? Number.POSITIVE_INFINITY : undefined });
  const count = cells.length;
  // Opened, or a stage added while open: the newest stage is in view.
  useLayoutEffect(() => {
    const row = rowRef.current;
    if (expanded && row) row.scrollLeft = row.scrollWidth;
  }, [expanded, count]);
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
  // A finished stage takes the story to its member's card (when that card is on this page).
  const reveal = (runId: string) => {
    const card = document.querySelector<HTMLElement>(`[data-testid="member-card"][data-run-id="${CSS.escape(runId)}"]`);
    if (!card) return;
    card.scrollIntoView({ block: "start", behavior: "smooth" });
    card.focus({ preventScroll: true });
  };
  const body = (cell: StageCell, color: number | null) => <>
    <span className="m5-stage-title">{color !== null && <span className="m5-role-dot" data-role-color={color} aria-hidden="true" />}<span className="m5-stage-name" title={title(cell)}><bdi>{title(cell)}</bdi></span>{cell.kind !== "next" && isLeadRole(cell.kind === "run" ? cell.roleId : "coordinator") && <span className="m5-role-lead" aria-label={t("m5.card.lead")}>★</span>}</span>
    <span className="m5-stage-meta">{meta(cell)}</span>
  </>;
  const stage = (cell: StageCell, index: number) => {
    const status = cell.kind === "run" ? cell.status : cell.kind === "next" ? "next" : "now";
    const color = cell.kind === "run" ? roleColorIndex(cell.roleId) : cell.kind === "next" ? null : roleColorIndex("coordinator");
    const key = cell.kind === "run" ? cell.runId : `${cell.kind}-${index}`;
    return <div key={key} role="listitem" className={`m5-stage m5-stage-${status}`} data-stage={cell.kind} aria-current={status === "now" ? "step" : undefined}>
      {expanded && cell.kind === "run"
        ? <button type="button" className="m5-stage-jump" aria-label={t("m5.stage.jump", { name: title(cell), status: meta(cell) })} onClick={() => reveal(cell.runId)}>{body(cell, color)}</button>
        : body(cell, color)}
    </div>;
  };
  return <div className={`m5-stage-bar${expanded ? " m5-stage-bar-all" : ""}`} data-testid="stage-bar">
    {hidden > 0 && <button type="button" className="m5-stage-more" aria-expanded="false" title={t("m5.stage.showAll", { count: hidden })} aria-label={t("m5.stage.showAll", { count: hidden })} onClick={() => setExpanded(true)}>+{hidden}</button>}
    {expanded && <button type="button" className="m5-stage-more m5-stage-fold" aria-expanded="true" title={t("m5.stage.fold")} onClick={() => setExpanded(false)}>{t("m5.stage.fold")}</button>}
    <div ref={rowRef} className="m5-stage-row" role="list" aria-label={t("m5.stage.label")}>{cells.map(stage)}</div>
    <p className="m5-sr-only" aria-live="polite">{announce}</p>
  </div>;
}
