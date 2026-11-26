import type { ChatMergeState } from "./contracts/project-catalog-v1.ts";

/**
 * What a merge confirmation is about: the task's head and the base's tip as
 * read when the person opened the confirmation. The merge is sent with
 * exactly these (`expectedHead`/`expectedBase`), never with a later read, so
 * the engine refuses it if either moved after the person looked.
 */
export interface MergeConfirmation { head: string; baseTip: string }

/** A confirmation can start only from a mergeable read. */
export function mergeConfirmationFor(state: ChatMergeState | null): MergeConfirmation | null {
  if (state?.kind !== "gitWorktree" || !state.baseTip || state.blocked) return null;
  return { head: state.head, baseTip: state.baseTip };
}

/**
 * Whether an open confirmation still matches the latest read. Any change of
 * the task's head or the base's tip ends it: the person confirms again.
 * An unreadable state (null) leaves it to the engine's own check.
 */
export function mergeConfirmationStillValid(confirmation: MergeConfirmation, latest: ChatMergeState | null): boolean {
  if (latest === null) return true;
  return latest.kind === "gitWorktree" && latest.head === confirmation.head && latest.baseTip === confirmation.baseTip;
}

/**
 * The same idea for a copy task's "apply to my folder": the confirmation is
 * about the workspace as it stood (how many files differ from what was
 * applied). If that changes while the confirmation is open, it ends.
 */
export interface ApplyConfirmation { applied: boolean; pending: number; total: number }
export function applyConfirmationStillValid(confirmation: ApplyConfirmation, latest: ApplyConfirmation | null): boolean {
  if (latest === null) return true;
  return latest.applied === confirmation.applied && latest.pending === confirmation.pending && latest.total === confirmation.total;
}
