import { canAddWorker, restoreWorker, type RemovedWorker, type TeamDraft } from "./team-draft.ts";

/**
 * The team editor's multi-step actions, kept pure so the review findings of
 * PR #8 stay reproducible in tests (I2, I3, I4).
 */

export type UndoConflict = "sameId" | "full";

/**
 * Undo the newest removal. When it cannot be applied — a starting role was
 * added back from its template under the same ID, or the team is full — the
 * team and the removal record stay as they are and the conflict says why
 * (review I2: the record used to be dropped while nothing came back).
 */
export function undoLastRemoval(team: TeamDraft, removed: readonly RemovedWorker[]): { team: TeamDraft; removed: readonly RemovedWorker[]; conflict: UndoConflict | null } {
  const entry = removed[removed.length - 1];
  if (!entry) return { team, removed, conflict: null };
  if (team.roles.some(role => role.roleId === entry.profile.roleId)) return { team, removed, conflict: "sameId" };
  if (!canAddWorker(team)) return { team, removed, conflict: "full" };
  return { team: restoreWorker(team, entry), removed: removed.slice(0, -1), conflict: null };
}

/**
 * Loading the app defaults replaces the whole draft: with unsaved changes it
 * asks first, like closing or reloading (review I3).
 */
export function appDefaultsLoadStep(dirty: boolean): "confirm" | "load" {
  return dirty ? "confirm" : "load";
}

/** What '앱 기본값으로도 저장' actually saved. */
export type CombinedSaveOutcome = "both" | "projectOnly" | "none";

/**
 * The project's team first: if it is not saved, the app defaults are left
 * alone, so nothing global changes for a team the project does not have.
 * Only when both are saved does the screen say so (review I4).
 */
export async function saveProjectThenAppDefaults(saveProject: () => Promise<boolean>, saveAppDefaults: () => Promise<void>): Promise<CombinedSaveOutcome> {
  if (!(await saveProject())) return "none";
  try {
    await saveAppDefaults();
    return "both";
  } catch {
    return "projectOnly";
  }
}
