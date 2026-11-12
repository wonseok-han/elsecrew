import {
  G5_COORDINATOR_ROLE_ID, G5_ROLES_MAX_V3, roleSettingsAsV3, validateProjectRoleSettingsUpdateRequestV1,
  type ProjectRoleSettingsV1, type RoleDefinitionV3, type RoleProfileV1,
} from "./contracts/run-recovery-v1.ts";

/** Editable V3 team. Profiles keep the 5-key shape; definitions are parallel. */
export interface TeamDraft { roles: readonly RoleProfileV1[]; roleDefinitions: readonly RoleDefinitionV3[] }

export function teamDraftFrom(settings: Pick<ProjectRoleSettingsV1, "roles" | "roleDefinitions">): TeamDraft {
  return roleSettingsAsV3(settings);
}

export function sameTeam(left: TeamDraft, right: TeamDraft): boolean {
  return JSON.stringify(left) === JSON.stringify(right);
}

/** First unused `member-N`; IDs are opaque and never derived from user text. */
export function nextWorkerId(draft: TeamDraft): string {
  for (let index = 1; ; index += 1) {
    const id = `member-${index}`;
    if (!draft.roles.some(role => role.roleId === id)) return id;
  }
}

export function canAddWorker(draft: TeamDraft): boolean { return draft.roles.length < G5_ROLES_MAX_V3; }
export function canRemoveWorker(draft: TeamDraft, roleId: string): boolean {
  return roleId !== G5_COORDINATOR_ROLE_ID && draft.roles.length > 2 && draft.roles.some(role => role.roleId === roleId);
}

/** New members start read-only with the coordinator's agent and model. */
export function addWorker(draft: TeamDraft, name: string): TeamDraft {
  if (!canAddWorker(draft)) return draft;
  const roleId = nextWorkerId(draft);
  const base = draft.roles[0];
  return {
    roles: [...draft.roles, { roleId, runtime: base.runtime, modelSelector: base.modelSelector, reasoningEffort: base.reasoningEffort, permission: "readOnly" }],
    roleDefinitions: [...draft.roleDefinitions, { roleId, name, responsibility: "" }],
  };
}

export function removeWorker(draft: TeamDraft, roleId: string): TeamDraft {
  if (!canRemoveWorker(draft, roleId)) return draft;
  return { roles: draft.roles.filter(role => role.roleId !== roleId), roleDefinitions: draft.roleDefinitions.filter(item => item.roleId !== roleId) };
}

export function updateProfile(draft: TeamDraft, roleId: string, change: Partial<Omit<RoleProfileV1, "roleId">>): TeamDraft {
  // The coordinator stays read-only regardless of the requested change.
  return { ...draft, roles: draft.roles.map(role => role.roleId !== roleId ? role : { ...role, ...change, ...(roleId === G5_COORDINATOR_ROLE_ID ? { permission: "readOnly" as const } : {}) }) };
}

export function updateDefinition(draft: TeamDraft, roleId: string, change: Partial<Pick<RoleDefinitionV3, "name" | "responsibility">>): TeamDraft {
  return { ...draft, roleDefinitions: draft.roleDefinitions.map(item => item.roleId === roleId ? { ...item, ...change } : item) };
}

/** Same contract the bridge and sidecar enforce, evaluated before saving. */
export function isValidTeamDraft(draft: TeamDraft): boolean {
  try {
    validateProjectRoleSettingsUpdateRequestV1({ projectId: "draft", expectedRevision: 1, roles: draft.roles, roleDefinitions: draft.roleDefinitions });
    return true;
  } catch { return false; }
}
