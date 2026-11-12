import type { MessageKey } from "../locales/ko.ts";
import { G5_DEFAULT_ROLE_DEFINITIONS_V3, G5_ROLE_IDS_V2, type LegacyRoleIdV1, type RoleDefinitionV3 } from "./contracts/run-recovery-v1.ts";

export type RoleLabelKey = `m5.roles.${LegacyRoleIdV1}`;

export function isLegacyRoleId(roleId: string): roleId is LegacyRoleIdV1 {
  return (G5_ROLE_IDS_V2 as readonly string[]).includes(roleId);
}

export function legacyRoleLabelKey(roleId: LegacyRoleIdV1): RoleLabelKey & MessageKey {
  return `m5.roles.${roleId}`;
}

/**
 * Display name for a role. A stored V3 name wins unless it is still the
 * locale-independent default of a legacy template ID; then the localized label
 * is shown. V1/V2 records have no definitions and always use the legacy label.
 */
export function roleLabel(roleId: string, definitions: readonly RoleDefinitionV3[] | undefined | null, translate: (key: MessageKey) => string): string {
  const stored = definitions?.find(definition => definition.roleId === roleId)?.name;
  if (isLegacyRoleId(roleId)) {
    const seed = G5_DEFAULT_ROLE_DEFINITIONS_V3.find(definition => definition.roleId === roleId)?.name;
    if (!stored || stored === seed) return translate(legacyRoleLabelKey(roleId));
  }
  return stored ?? roleId;
}
