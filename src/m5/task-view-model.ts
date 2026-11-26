import type { ChatStateV1 } from "./contracts/run-recovery-v1.ts";
import { changesStatus, phaseIsWorking, type TaskKind, type TaskPhase } from "./task-status.ts";

/**
 * What the task layout says around the single task status (ADE 1차-나,
 * PR #5 review). The status itself stays in task-status.ts; these rules
 * only decide which sentence and which place applies.
 */

/**
 * The team is actually working (review #3). `attention` covers both a
 * permission request while the team runs and a task stopped for recovery or
 * a check (`needsAttention`, `recoveryRequired`): only the first is work in
 * progress. Read from the chat state, never from the phase.
 */
const ACTIVE: readonly ChatStateV1[] = ["draft", "preparing", "running", "stopping", "applying"];
export function teamWorking(state: ChatStateV1 | null | undefined, starting = false): boolean {
  return starting || (state !== null && state !== undefined && ACTIVE.includes(state));
}

/**
 * The changes tab's one line (review #3, #4). Nothing about the user's
 * folder is claimed while the standing is unread or could not be read.
 * Git tasks with a known standing keep their own line (ChangesMergeStatus).
 */
export type ChangesLine = "working" | "attention" | "loading" | "failed" | "notYet" | "done" | "changedSince" | null;
export function changesLine(input: { phase: TaskPhase; working: boolean; kind: TaskKind | null; known: boolean; failed: boolean; changeCount: number }): ChangesLine {
  const { phase, working, known, failed, changeCount } = input;
  if (working) return changeCount > 0 ? "working" : null;
  if (phaseIsWorking(phase)) return changeCount > 0 ? "attention" : null;
  if (failed) return "failed";
  if (!known) return changeCount > 0 ? "loading" : null;
  const status = changesStatus(phase, changeCount);
  return status === "working" ? null : status;
}

/**
 * The result box (review #3): "put it in here once the team is done" only
 * while the team works; a task that needs a check says so instead; else the
 * result bar view decides.
 */
export type ResultBoxMode = "waiting" | "attention" | "view" | "hidden";
export function resultBoxMode(input: { phase: TaskPhase | null; working: boolean; switching: boolean; hasView: boolean }): ResultBoxMode {
  if (input.switching) return input.hasView ? "view" : "hidden";
  if (input.working) return "waiting";
  if (input.phase !== null && phaseIsWorking(input.phase)) return "attention";
  return input.hasView ? "view" : "hidden";
}

/** Menus that close on Escape and outside clicks (one list for every handler). */
export const MENU_SELECTOR = ".m5-project-menu, .m5-project-guide, .m5-project-overflow, .m5-diff-options, .m5-notifications, .m5-project-switcher, .m5-task-menu";

/**
 * What one Escape closes (review #5): the innermost thing only. An open
 * menu or dialog first, then the sheet, then the drawer. Something that
 * already handled the key (Monaco's suggestions, a menu) ends it.
 */
export type EscapeTarget = "none" | "menu" | "sheet" | "drawer";
export function escapeTarget(input: { handled: boolean; menuOpen: boolean; dialogOpen: boolean; sheetOpen: boolean; drawerOpen: boolean }): EscapeTarget {
  if (input.handled || input.dialogOpen) return "none";
  if (input.menuOpen) return "menu";
  if (input.sheetOpen) return "sheet";
  if (input.drawerOpen) return "drawer";
  return "none";
}

/**
 * A permission request waits but the card cannot be reached: the sheet
 * covers the story, or the drawer and its scrim do (review #1). Then the
 * sheet and the drawer say so and offer to go to it.
 */
export function approvalCovered(input: { waiting: boolean; sheetOpen: boolean; drawerOpen: boolean }): boolean {
  return input.waiting && (input.sheetOpen || input.drawerOpen);
}

/** Showing the results (a tab or the result box) opens what holds them first (review #2). */
export function revealResults(drawerUsed: boolean): { drawerOpen: boolean; rightCollapsed: boolean } {
  return { drawerOpen: drawerUsed, rightCollapsed: false };
}
