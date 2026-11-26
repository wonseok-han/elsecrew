import {
  G5_COORDINATOR_ROLE_ID, G5_DEFAULT_ROLE_DEFINITIONS_V3, G5_ROLE_NAME_MAX_BYTES_V3, G5_ROLES_MAX_V3, roleNameKey, roleSettingsAsV3, sameJsonValue,
  utf8ByteLength, validateProjectRoleSettingsUpdateRequestV1,
  type ProjectRoleSettingsV1, type RoleDefinitionV3, type RoleProfileV1,
} from "./contracts/run-recovery-v1.ts";

/** Editable V3 team. Profiles keep the 5-key shape; definitions are parallel. */
export interface TeamDraft { roles: readonly RoleProfileV1[]; roleDefinitions: readonly RoleDefinitionV3[] }

export function teamDraftFrom(settings: Pick<ProjectRoleSettingsV1, "roles" | "roleDefinitions">): TeamDraft {
  return roleSettingsAsV3(settings);
}

export function sameTeam(left: TeamDraft, right: TeamDraft): boolean {
  return sameJsonValue(left, right);
}

/**
 * A new member's ID: `member-` and six random base36 letters, never one the
 * team has or had (`taken`: IDs loaded or seen while editing). IDs are opaque
 * and never derived from user text. A random ID is not reused later, so a new
 * role never inherits an old role's report file `docs/elsecrew/<id>.md` (R2).
 */
export function newMemberId(taken: Iterable<string>, random: () => number = secureRandom): string {
  const used = new Set(taken);
  for (;;) {
    let suffix = "";
    for (let index = 0; index < 6; index++) suffix += Math.floor(random() * 36).toString(36);
    const id = `member-${suffix}`;
    if (!used.has(id)) return id;
  }
}

function secureRandom(): number {
  const value = new Uint32Array(1);
  globalThis.crypto.getRandomValues(value);
  return value[0]! / 0x1_0000_0000;
}

export function canAddWorker(draft: TeamDraft): boolean { return draft.roles.length < G5_ROLES_MAX_V3; }
export function canRemoveWorker(draft: TeamDraft, roleId: string): boolean {
  return roleId !== G5_COORDINATOR_ROLE_ID && draft.roles.length > 2 && draft.roles.some(role => role.roleId === roleId);
}

/** What a new member starts as; the ID is chosen by the caller. */
export interface WorkerSeed {
  roleId: string;
  name: string;
  responsibility: string;
  permission: RoleProfileV1["permission"];
  /** Agent, model and effort; omitted, the coordinator's are copied. */
  profile?: Pick<RoleProfileV1, "runtime" | "modelSelector" | "reasoningEffort">;
}

/**
 * Inserts a member at `index` (1 … roles.length; the coordinator keeps 0).
 * Unchanged when the team is full or the ID is already in it.
 */
export function insertWorker(draft: TeamDraft, index: number, seed: WorkerSeed): TeamDraft {
  if (!canAddWorker(draft) || draft.roles.some(role => role.roleId === seed.roleId) || seed.roleId === G5_COORDINATOR_ROLE_ID) return draft;
  const at = Math.min(Math.max(Math.trunc(index), 1), draft.roles.length);
  const base = seed.profile ?? draft.roles[0]!;
  const profile: RoleProfileV1 = { roleId: seed.roleId, runtime: base.runtime, modelSelector: base.modelSelector, reasoningEffort: base.reasoningEffort, permission: seed.permission };
  const definition: RoleDefinitionV3 = { roleId: seed.roleId, name: seed.name, responsibility: seed.responsibility };
  return {
    roles: [...draft.roles.slice(0, at), profile, ...draft.roles.slice(at)],
    roleDefinitions: [...draft.roleDefinitions.slice(0, at), definition, ...draft.roleDefinitions.slice(at)],
  };
}

/** Appends a blank member: read-only until the person chooses otherwise (Q4). */
export function addWorker(draft: TeamDraft, name: string, roleId = newMemberId(draft.roles.map(role => role.roleId))): TeamDraft {
  return insertWorker(draft, draft.roles.length, { roleId, name, responsibility: "", permission: "readOnly" });
}

/**
 * Moves the member at `from` to `to` (both 1 … roles.length - 1). The
 * coordinator never moves and nothing moves above it. List order is only the
 * order shown; the coordinator decides who works when (Q1).
 */
export function moveWorker(draft: TeamDraft, from: number, to: number): TeamDraft {
  const last = draft.roles.length - 1;
  if (!Number.isInteger(from) || !Number.isInteger(to) || from < 1 || from > last || from === to) return draft;
  const target = Math.min(Math.max(to, 1), last);
  if (target === from) return draft;
  const move = <T>(items: readonly T[]): T[] => {
    const next = [...items];
    const [item] = next.splice(from, 1);
    next.splice(target, 0, item!);
    return next;
  };
  return { roles: move(draft.roles), roleDefinitions: move(draft.roleDefinitions) };
}

/** A copy of a member right below it, under a new ID and a name not yet used. */
export function duplicateWorker(draft: TeamDraft, roleId: string, name: string, newId: string): TeamDraft {
  const index = draft.roles.findIndex(role => role.roleId === roleId);
  if (index < 1) return draft;
  const role = draft.roles[index]!;
  const definition = draft.roleDefinitions[index]!;
  return insertWorker(draft, index + 1, { roleId: newId, name, responsibility: definition.responsibility, permission: role.permission, profile: role });
}

export function removeWorker(draft: TeamDraft, roleId: string): TeamDraft {
  if (!canRemoveWorker(draft, roleId)) return draft;
  return { roles: draft.roles.filter(role => role.roleId !== roleId), roleDefinitions: draft.roleDefinitions.filter(item => item.roleId !== roleId) };
}

/** A removed member and where it was, so the removal can be undone before saving. */
export interface RemovedWorker { index: number; profile: RoleProfileV1; definition: RoleDefinitionV3 }

export function removedWorker(draft: TeamDraft, roleId: string): RemovedWorker | null {
  const index = draft.roles.findIndex(role => role.roleId === roleId);
  return index < 1 ? null : { index, profile: draft.roles[index]!, definition: draft.roleDefinitions[index]! };
}

/** Puts a removed member back where it was (or as near as the team allows). */
export function restoreWorker(draft: TeamDraft, removed: RemovedWorker): TeamDraft {
  const { profile, definition } = removed;
  return insertWorker(draft, removed.index, { roleId: profile.roleId, name: definition.name, responsibility: definition.responsibility, permission: profile.permission, profile });
}

export function updateProfile(draft: TeamDraft, roleId: string, change: Partial<Omit<RoleProfileV1, "roleId">>): TeamDraft {
  // The coordinator stays read-only regardless of the requested change.
  return { ...draft, roles: draft.roles.map(role => role.roleId !== roleId ? role : { ...role, ...change, ...(roleId === G5_COORDINATOR_ROLE_ID ? { permission: "readOnly" as const } : {}) }) };
}

export function updateDefinition(draft: TeamDraft, roleId: string, change: Partial<Pick<RoleDefinitionV3, "name" | "responsibility">>): TeamDraft {
  return { ...draft, roleDefinitions: draft.roleDefinitions.map(item => item.roleId === roleId ? { ...item, ...change } : item) };
}

/** The coordinator's name and description as the app ships them (Q7). */
export function coordinatorDefaults(): Pick<RoleDefinitionV3, "name" | "responsibility"> {
  const seed = G5_DEFAULT_ROLE_DEFINITIONS_V3[0]!;
  return { name: seed.name, responsibility: seed.responsibility };
}

/**
 * `base` if no role is called that (ignoring spaces and case), else
 * `base 2`, `base 3` … — shortened so the name stays within 64 bytes.
 */
export function uniqueRoleName(draft: TeamDraft, base: string, reserved: readonly string[] = []): string {
  const used = new Set([...draft.roleDefinitions.map(item => item.name), ...reserved].map(roleNameKey));
  const fit = (text: string, suffix: string) => {
    let chars = Array.from(text);
    while (chars.length > 1 && utf8ByteLength(chars.join("") + suffix) > G5_ROLE_NAME_MAX_BYTES_V3) chars = chars.slice(0, -1);
    return chars.join("").trimEnd() + suffix;
  };
  for (let number = 1; ; number++) {
    const name = fit(base, number === 1 ? "" : ` ${number}`);
    if (!used.has(roleNameKey(name))) return name;
  }
}

/** Same contract the bridge and sidecar enforce, evaluated before saving. */
export function isValidTeamDraft(draft: TeamDraft): boolean {
  try {
    validateProjectRoleSettingsUpdateRequestV1({ projectId: "draft", expectedRevision: 1, roles: draft.roles, roleDefinitions: draft.roleDefinitions });
    return true;
  } catch { return false; }
}

/** What changed between the loaded team and the draft, for the unsaved summary. */
export interface TeamChanges { added: number; removed: number; moved: boolean; edited: number }

export function teamChanges(before: TeamDraft, after: TeamDraft): TeamChanges {
  const beforeIds = before.roles.map(role => role.roleId);
  const afterIds = after.roles.map(role => role.roleId);
  const kept = new Set(beforeIds.filter(id => afterIds.includes(id)));
  const order = (ids: readonly string[]) => ids.filter(id => kept.has(id)).join("\u0000");
  let edited = 0;
  for (const id of kept) {
    const left = { role: before.roles.find(role => role.roleId === id), definition: before.roleDefinitions.find(item => item.roleId === id) };
    const right = { role: after.roles.find(role => role.roleId === id), definition: after.roleDefinitions.find(item => item.roleId === id) };
    if (!sameJsonValue(left, right)) edited++;
  }
  return {
    added: afterIds.filter(id => !kept.has(id)).length,
    removed: beforeIds.filter(id => !kept.has(id)).length,
    moved: order(beforeIds) !== order(afterIds),
    edited,
  };
}
