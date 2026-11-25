/**
 * A settings form's draft next to the values it was loaded with.
 *
 * "Unsaved changes" means the person changed something: the draft differs
 * from the loaded values. Corrections the screen makes by itself (a role
 * without a model getting its provider's recommendation, a renamed model) are
 * applied to both sides, so opening and closing a settings page never asks
 * whether to discard changes nobody made.
 *
 * Pure and generic so every settings editor (the role editor included) uses
 * the same rule.
 */
export interface SettingsDraft<T> {
  /** What is saved now, exactly as loaded. */
  readonly loaded: T;
  /** What was loaded, plus the screen's own corrections. */
  readonly baseline: T;
  /** What the form shows and saves. */
  readonly current: T;
}

export function loadSettingsDraft<T>(loaded: T): SettingsDraft<T> {
  return { loaded, baseline: loaded, current: loaded };
}

/** A change the person made. */
export function editSettingsDraft<T>(draft: SettingsDraft<T>, edit: (current: T) => T): SettingsDraft<T> {
  const current = edit(draft.current);
  return current === draft.current ? draft : { loaded: draft.loaded, baseline: draft.baseline, current };
}

/**
 * A correction the screen makes by itself. It is applied to the loaded values
 * too, so it never counts as an unsaved change; the person's own edits stay.
 */
export function correctSettingsDraft<T>(draft: SettingsDraft<T>, correct: (value: T) => T): SettingsDraft<T> {
  const baseline = correct(draft.baseline);
  const current = correct(draft.current);
  return baseline === draft.baseline && current === draft.current ? draft : { loaded: draft.loaded, baseline, current };
}

/** True only when the person changed something since loading. */
export function settingsDraftDirty<T>(draft: SettingsDraft<T>, same: (a: T, b: T) => boolean): boolean {
  return !same(draft.baseline, draft.current);
}

/**
 * Saving changes what is stored: possible whenever the form differs from what
 * was loaded, the screen's own corrections included (a recommended model it
 * filled in can be kept by saving). Leaving without saving only asks when
 * `settingsDraftDirty` says the person changed something.
 */
export function settingsDraftSavable<T>(draft: SettingsDraft<T>, same: (a: T, b: T) => boolean): boolean {
  return !same(draft.loaded, draft.current);
}
