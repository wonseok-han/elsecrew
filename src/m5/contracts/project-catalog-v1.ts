import { validateChatRecordV1, validateProjectRoleSettingsV1, validateProjectRoleSettingsUpdateRequestV1, type ProjectRoleSettingsV1, type RoleProfileV1, type ChatRecordV1 } from "./run-recovery-v1.ts";
import { validateProjectIdentityV1, type ProjectIdentityV1 } from "./project-v1.ts";

export const PROJECT_CATALOG_METHOD = "project.catalog";
export const PROJECT_CATALOG_CHANNEL = "m5:project:catalog";
export interface PreparationInspection { projectId: string; worktreeId: string; workspaceIdentity: string; lockId: string; projectRevision: number; lockRevision: number }
export interface ApplicationStatus { projectId: string; applicationId: string | null; state: "none" | "prepared" | "applying" | "applied" | "restored" | "abandoned"; currentIndex: number | null; total: number; running: boolean; failure: {code: string; detail: string} | null }
export interface ApplicationHistoryEntry { applicationId: string; state: "applied" | "restored" | "abandoned"; total: number }
export interface ApplicationPreview { worktreeId: string; total: number; changes: { path: string; kind: "added" | "modified" | "deleted" }[] }
export interface ApplicationPreviewPage extends ApplicationPreview { offset: number; fingerprint: string }
export interface BackupPreparation { reviewId: string; fingerprint: string }
export interface ApplicationRetirementInspection { applicationId: string; fingerprint: string; state: "applied" | "restored" | "abandoned" }
export interface BackupCleanupInspection { reviewId: string; fingerprint: string; filePresent: boolean; state: "available" | "pending" | "deleted" }
export interface BackupRetention { reviewId: string; state: "preparing" | "ready" | "abandoned"; retainedFor: "currentApplication" | "applicationHistory" | "unfinishedPreparation" | "preparedBackup" | "abandonedPreparation"; cleanupState: "none" | "pending" | "deleted" }
export interface BackupStorageUsage { usedBytes: number; limitBytes: number; remainingBytes: number }
export interface ChatDiscardAddress { chatId: string; taskId: string; projectId: string; worktreeId: string; workspaceIdentity: string }
export interface ChatDiscardInspection extends ChatDiscardAddress { chatRevision: number }
export type AppRoleDefaults = Omit<ProjectRoleSettingsV1, "projectId">;
export interface GitTrustIdentity {
  installationPath: string; volumePath: string; directoryId: string;
  commonName: string; organization: string | null;
}
export interface GitTrustInspection { identity: GitTrustIdentity; trusted: boolean; executionEnabled: false }
export function validateGitTrustIdentity(value: unknown): GitTrustIdentity {
  const identity = object(value);
  keys(identity, ["installationPath", "volumePath", "directoryId", "commonName", "organization"]);
  for (const key of ["installationPath", "volumePath", "directoryId", "commonName", "organization"]) {
    const text = identity[key];
    if (key === "organization" && text === null) continue;
    if (typeof text !== "string" || text.length === 0 || text.length > (key === "commonName" || key === "organization" ? 4096 : 32768) || /[\u0000-\u001f\u007f]/.test(text)) throw new Error("invalid Git trust identity");
  }
  return identity as unknown as GitTrustIdentity;
}
function validateGitTrustInspection(value: unknown): GitTrustInspection {
  const inspection = object(value);
  keys(inspection, ["identity", "trusted", "executionEnabled"]);
  validateGitTrustIdentity(inspection.identity);
  if (typeof inspection.trusted !== "boolean" || inspection.executionEnabled !== false) throw new Error("invalid Git trust inspection");
  return inspection as unknown as GitTrustInspection;
}
export type TeamTaskCancelRequest = ChatDiscardAddress & { action: "cancelTeamTask"; triggerMessageId: string };
export type AutomationPermissions = { files: boolean; commands: boolean };
export type ProjectCatalogRequest =
  | { action: "getAutomationPermissions"; projectId: string }
  | { action: "setAutomationPermissions"; projectId: string; permissions: AutomationPermissions; confirmed: true }
  | { action: "getProviderConnections" }
  | { action: "setProviderConnection"; runtime: "codex" | "claude-code"; connected: boolean }
  | TeamTaskCancelRequest
  | { action: "inspectGitTrust" }
  | { action: "confirmGitTrust"; identity: GitTrustIdentity; confirmed: true }
  | { action: "revokeGitTrust" }
  | { action: "getAppRoleDefaults" }
  | { action: "updateAppRoleDefaults"; expectedRevision: number; roles: readonly RoleProfileV1[] }
  | ({ action: "inspectChatDiscard" } & ChatDiscardAddress)
  | { action: "discardChatWorkspace"; inspection: ChatDiscardInspection; confirmed: true }
  | { action: "removeProject"; projectId: string }
  | { action: "restoreChats"; projectId: string }
  | { action: "removeChat"; projectId: string; chatId: string }
  | { action: "list"; cursor: string | null } | { action: "reopen"; projectId: string } | { action: "releaseView"; projectId: string }
  | { action: "applicationCapabilities" }
  | { action: "inspectApplicationRetirement"; projectId: string; applicationId: string }
  | { action: "retireApplication"; projectId: string; applicationId: string; fingerprint: string }
  | { action: "backupStorageUsage" }
  | { action: "setBackupStorageLimit"; expectedLimitBytes: number; limitBytes: number }
  | { action: "backupPreparations"; projectId: string }
  | { action: "backupRetention"; projectId: string }
  | { action: "inspectBackupCleanup"; projectId: string; reviewId: string }
  | { action: "cleanupBackup"; projectId: string; reviewId: string; fingerprint: string }
  | { action: "abandonBackupPreparation"; projectId: string; reviewId: string; fingerprint: string }
  | { action: "startApplication"; projectId: string; chatId: string; confirmationId: string }
  | { action: "restoreApplication"; projectId: string; applicationId: string; fingerprint: string }
  | { action: "restorationPreview"; projectId: string; applicationId: string; offset: number; fingerprint: string | null }
  | { action: "cancelApplication"; projectId: string; applicationId: string }
  | { action: "openApplicationConfirmation"; projectId: string; chatId: string; fingerprint: string }
  | { action: "cancelApplicationConfirmation"; confirmationId: string }
  | { action: "applicationPreviewPage"; projectId: string; chatId: string | null; offset: number; fingerprint: string | null }
  | { action: "inspectPreparation"; projectId: string }
  | { action: "applicationStatus"; projectId: string }
  | { action: "chatApplicationStatus"; projectId: string; chatId: string }
  | { action: "applicationHistory"; projectId: string }
  | { action: "applicationPreview"; projectId: string }
  | { action: "chatApplicationPreview"; projectId: string; chatId: string }
  | { action: "prepareApplicationWorkspace"; projectId: string }
  | { action: "abandonPreparation"; inspection: PreparationInspection };
export interface ProjectCatalogEntry { projectId: string; canonicalPath: string }
export type ProjectCatalogResult =
  | { schemaVersion: 1; action: "getAutomationPermissions" | "setAutomationPermissions"; projectId: string; permissions: AutomationPermissions }
  | { schemaVersion: 1; action: "getProviderConnections" | "setProviderConnection"; connections: { codex: boolean; "claude-code": boolean } }
  | { schemaVersion: 1; action: "cancelTeamTask"; chatId: string; triggerMessageId: string; status: "stopping" | "stopped" }
  | { schemaVersion: 1; action: "inspectGitTrust" | "confirmGitTrust"; inspection: GitTrustInspection }
  | { schemaVersion: 1; action: "revokeGitTrust"; revoked: true }
  | { schemaVersion: 1; action: "getAppRoleDefaults" | "updateAppRoleDefaults"; settings: AppRoleDefaults }
  | { schemaVersion: 1; action: "inspectChatDiscard"; inspection: ChatDiscardInspection }
  | { schemaVersion: 1; action: "discardChatWorkspace"; chat: ChatRecordV1 }
  | { schemaVersion: 1; action: "applicationCapabilities"; enabled: boolean }
  | { schemaVersion: 1; action: "inspectApplicationRetirement"; projectId: string; enabled: boolean; inspection: ApplicationRetirementInspection }
  | { schemaVersion: 1; action: "retireApplication"; projectId: string; applicationId: string }
  | { schemaVersion: 1; action: "backupStorageUsage"; usage: BackupStorageUsage }
  | { schemaVersion: 1; action: "setBackupStorageLimit"; limitBytes: number }
  | { schemaVersion: 1; action: "backupPreparations"; projectId: string; entries: BackupPreparation[] }
  | { schemaVersion: 1; action: "backupRetention"; projectId: string; entries: BackupRetention[] }
  | { schemaVersion: 1; action: "inspectBackupCleanup"; projectId: string; enabled: boolean; inspection: BackupCleanupInspection }
  | { schemaVersion: 1; action: "cleanupBackup"; projectId: string; reviewId: string }
  | { schemaVersion: 1; action: "abandonBackupPreparation"; projectId: string; reviewId: string }
  | { schemaVersion: 1; action: "startApplication"; projectId: string; chatId: string; applicationId: string }
  | { schemaVersion: 1; action: "restoreApplication"; projectId: string; applicationId: string }
  | { schemaVersion: 1; action: "restorationPreview"; projectId: string; applicationId: string; state: "prepared" | "applying" | "applied"; page: ApplicationPreviewPage }
  | { schemaVersion: 1; action: "cancelApplication"; projectId: string; applicationId: string }
  | { schemaVersion: 1; action: "openApplicationConfirmation"; projectId: string; chatId: string; confirmationId: string; worktreeId: string; total: number; expiresInSeconds: number }
  | { schemaVersion: 1; action: "cancelApplicationConfirmation"; confirmationId: string }
  | { schemaVersion: 1; action: "applicationPreviewPage"; projectId: string; chatId: string | null; page: ApplicationPreviewPage }
  | { schemaVersion: 1; action: "chatApplicationPreview"; projectId: string; chatId: string; preview: ApplicationPreview }
  | { schemaVersion: 1; action: "prepareApplicationWorkspace"; projectId: string; worktreeId: string }
  | { schemaVersion: 1; action: "applicationPreview"; projectId: string; preview: ApplicationPreview }
  | { schemaVersion: 1; action: "applicationHistory"; projectId: string; entries: ApplicationHistoryEntry[] }
  | { schemaVersion: 1; action: "applicationStatus"; status: ApplicationStatus }
  | { schemaVersion: 1; action: "chatApplicationStatus"; chatId: string; status: ApplicationStatus }
  | { schemaVersion: 1; action: "inspectPreparation" | "abandonPreparation"; inspection: PreparationInspection }
  | { schemaVersion: 1; action: "list"; projects: ProjectCatalogEntry[]; nextCursor: string | null }
  | { schemaVersion: 1; action: "removeProject"; projectId: string }
  | { schemaVersion: 1; action: "restoreChats"; projectId: string }
  | { schemaVersion: 1; action: "removeChat"; projectId: string; chatId: string }
  | { schemaVersion: 1; action: "releaseView"; projectId: string }
  | { schemaVersion: 1; action: "reopen"; project: ProjectIdentityV1 };

function object(value: unknown): Record<string, unknown> {
  if (!value || typeof value !== "object" || Array.isArray(value)) throw new Error("invalid project catalog object");
  return value as Record<string, unknown>;
}
function keys(value: Record<string, unknown>, expected: string[]) {
  if (Object.keys(value).length !== expected.length || expected.some(key => !Object.prototype.hasOwnProperty.call(value, key))) throw new Error("invalid project catalog fields");
}
function id(value: unknown): string {
  if (typeof value !== "string" || !/^[A-Za-z0-9_-]{1,128}$/.test(value)) throw new Error("invalid catalog ID");
  return value;
}
function backupLimit(value: unknown) {
  if (!Number.isSafeInteger(value) || (value as number) < 1024 ** 3 || (value as number) > 1024 ** 4 || (value as number) % 1024 ** 3 !== 0) throw new Error("invalid backup storage limit");
}
export function validatePreparationInspection(value: unknown): PreparationInspection {
  const inspection = object(value);
  keys(inspection, ["projectId", "worktreeId", "workspaceIdentity", "lockId", "projectRevision", "lockRevision"]);
  for (const key of ["projectId", "worktreeId", "workspaceIdentity", "lockId"]) id(inspection[key]);
  if (inspection.lockId !== `lock_${inspection.worktreeId}`) throw new Error("preparation lock mismatch");
  for (const key of ["projectRevision", "lockRevision"]) if (!Number.isSafeInteger(inspection[key]) || (inspection[key] as number) < 1) throw new Error("invalid preparation revision");
  return inspection as unknown as PreparationInspection;
}
export function validateChatDiscardInspection(value: unknown): ChatDiscardInspection {
  const inspection = object(value);
  keys(inspection, ["chatId", "taskId", "projectId", "worktreeId", "workspaceIdentity", "chatRevision"]);
  for (const key of ["chatId", "taskId", "projectId", "worktreeId", "workspaceIdentity"]) id(inspection[key]);
  if (!Number.isSafeInteger(inspection.chatRevision) || (inspection.chatRevision as number) < 1) throw new Error("invalid chat revision");
  return inspection as unknown as ChatDiscardInspection;
}
export function validateProjectCatalogRequest(value: unknown): ProjectCatalogRequest {
  const request = object(value);
  if (request.action === "getAutomationPermissions" || request.action === "setAutomationPermissions") {
    keys(request, request.action === "setAutomationPermissions" ? ["action","projectId","permissions","confirmed"] : ["action","projectId"]);
    id(request.projectId);
    if (request.action === "setAutomationPermissions") {
      const permissions = object(request.permissions); keys(permissions,["files","commands"]);
      if (typeof permissions.files !== "boolean" || typeof permissions.commands !== "boolean" || request.confirmed !== true) throw new Error("invalid automation consent");
    }
  } else if (request.action === "cancelTeamTask") {
    keys(request, ["action", "chatId", "taskId", "projectId", "worktreeId", "workspaceIdentity", "triggerMessageId"]);
    for (const key of ["chatId", "taskId", "projectId", "worktreeId", "workspaceIdentity", "triggerMessageId"]) id(request[key]);
  } else if (request.action === "inspectGitTrust" || request.action === "revokeGitTrust") {
    keys(request, ["action"]);
  } else if (request.action === "confirmGitTrust") {
    keys(request, ["action", "identity", "confirmed"]);
    validateGitTrustIdentity(request.identity);
    if (request.confirmed !== true) throw new Error("Git trust requires explicit confirmation");
  } else if (request.action === "getProviderConnections") {
    keys(request,["action"]);
  } else if (request.action === "setProviderConnection") {
    keys(request,["action","runtime","connected"]);
    if ((request.runtime !== "codex" && request.runtime !== "claude-code") || typeof request.connected !== "boolean") throw new Error("invalid provider connection");
  } else if (request.action === "getAppRoleDefaults") {
    keys(request, ["action"]);
  } else if (request.action === "updateAppRoleDefaults") {
    keys(request, ["action", "expectedRevision", "roles"]);
    validateProjectRoleSettingsUpdateRequestV1({projectId: "app_defaults", expectedRevision: request.expectedRevision, roles: request.roles});
  } else if (request.action === "inspectChatDiscard") {
    keys(request, ["action", "chatId", "taskId", "projectId", "worktreeId", "workspaceIdentity"]);
    for (const key of ["chatId", "taskId", "projectId", "worktreeId", "workspaceIdentity"]) id(request[key]);
  } else if (request.action === "discardChatWorkspace") {
    keys(request, ["action", "inspection", "confirmed"]);
    validateChatDiscardInspection(request.inspection);
    if (request.confirmed !== true) throw new Error("workspace discard requires explicit confirmation");
  } else if (request.action === "applicationCapabilities" || request.action === "backupStorageUsage") {
    keys(request, ["action"]);
  } else if (request.action === "setBackupStorageLimit") {
    keys(request, ["action", "expectedLimitBytes", "limitBytes"]);
    backupLimit(request.expectedLimitBytes); backupLimit(request.limitBytes);
  } else if (request.action === "backupPreparations" || request.action === "backupRetention") {
    keys(request, ["action", "projectId"]); id(request.projectId);
  } else if (request.action === "inspectBackupCleanup") {
    keys(request, ["action", "projectId", "reviewId"]); id(request.projectId);
    if (typeof request.reviewId !== "string" || !/^review_[a-f0-9]{32}$/.test(request.reviewId)) throw new Error("invalid cleanup review");
  } else if (request.action === "cleanupBackup" || request.action === "abandonBackupPreparation") {
    keys(request, ["action", "projectId", "reviewId", "fingerprint"]); id(request.projectId);
    if (typeof request.reviewId !== "string" || !/^review_[a-f0-9]{32}$/.test(request.reviewId) || typeof request.fingerprint !== "string" || !/^[a-f0-9]{64}$/.test(request.fingerprint)) throw new Error("invalid backup preparation identity");
  } else if (request.action === "startApplication") {
    keys(request, ["action", "projectId", "chatId", "confirmationId"]); id(request.projectId); id(request.chatId);
    validateProjectCatalogRequest({action:"cancelApplicationConfirmation",confirmationId:request.confirmationId});
  } else if (request.action === "restoreApplication" || request.action === "retireApplication") {
    keys(request, ["action", "projectId", "applicationId", "fingerprint"]);
    if (typeof request.fingerprint !== "string" || !/^[a-f0-9]{64}$/.test(request.fingerprint)) throw new Error("invalid restoration fingerprint");
    validateProjectCatalogRequest({action:"cancelApplication",projectId:request.projectId,applicationId:request.applicationId});
  } else if (request.action === "restorationPreview") {
    keys(request, ["action", "projectId", "applicationId", "offset", "fingerprint"]);
    validateProjectCatalogRequest({action:"cancelApplication",projectId:request.projectId,applicationId:request.applicationId});
    validateProjectCatalogRequest({action:"applicationPreviewPage",projectId:request.projectId,chatId:null,offset:request.offset,fingerprint:request.fingerprint});
  } else if (request.action === "cancelApplication" || request.action === "inspectApplicationRetirement") {
    keys(request, ["action", "projectId", "applicationId"]); id(request.projectId);
    if (typeof request.applicationId !== "string" || !/^application_[a-f0-9]{32}$/.test(request.applicationId)) throw new Error("invalid application cancellation ID");
  } else if (request.action === "openApplicationConfirmation") {
    keys(request, ["action", "projectId", "chatId", "fingerprint"]); id(request.projectId); id(request.chatId);
    if (typeof request.fingerprint !== "string" || !/^[a-f0-9]{64}$/.test(request.fingerprint)) throw new Error("invalid confirmation fingerprint");
  } else if (request.action === "cancelApplicationConfirmation") {
    keys(request, ["action", "confirmationId"]);
    if (typeof request.confirmationId !== "string" || !/^confirmation_[a-f0-9]{32}$/.test(request.confirmationId)) throw new Error("invalid confirmation ID");
  } else if (request.action === "applicationPreviewPage") {
    keys(request, ["action", "projectId", "chatId", "offset", "fingerprint"]); id(request.projectId);
    if (request.chatId !== null) id(request.chatId);
    if (!Number.isSafeInteger(request.offset) || (request.offset as number) < 0 || (request.offset as number) > 100000 || (request.offset as number) % 100 !== 0 || (request.offset !== 0 && request.fingerprint === null) || (request.fingerprint !== null && (typeof request.fingerprint !== "string" || !/^[a-f0-9]{64}$/.test(request.fingerprint)))) throw new Error("invalid review cursor");
  } else if (request.action === "list") {
    keys(request, ["action", "cursor"]);
    if (request.cursor !== null) id(request.cursor);
  } else if (request.action === "removeProject" || request.action === "restoreChats" || request.action === "reopen" || request.action === "releaseView" || request.action === "inspectPreparation" || request.action === "applicationStatus" || request.action === "applicationHistory" || request.action === "applicationPreview" || request.action === "prepareApplicationWorkspace") {
    keys(request, ["action", "projectId"]); id(request.projectId);
  } else if (request.action === "removeChat" || request.action === "chatApplicationPreview" || request.action === "chatApplicationStatus") {
    keys(request, ["action", "projectId", "chatId"]); id(request.projectId); id(request.chatId);
  } else if (request.action === "abandonPreparation") {
    keys(request, ["action", "inspection"]); validatePreparationInspection(request.inspection);
  } else throw new Error("invalid project catalog action");
  return request as ProjectCatalogRequest;
}
export function validateProjectCatalogResult(value: unknown, input: ProjectCatalogRequest): ProjectCatalogResult {
  const request = validateProjectCatalogRequest(input);
  const result = object(value);
  if (result.schemaVersion !== 1 || result.action !== request.action || new TextEncoder().encode(JSON.stringify(value)).byteLength > 768 * 1024) throw new Error("invalid catalog response envelope");
  if (request.action === "getAutomationPermissions" || request.action === "setAutomationPermissions") {
    keys(result,["schemaVersion","action","projectId","permissions"]);
    const permissions = object(result.permissions); keys(permissions,["files","commands"]);
    if (result.projectId !== request.projectId || typeof permissions.files !== "boolean" || typeof permissions.commands !== "boolean") throw new Error("invalid automation permissions");
    if (request.action === "setAutomationPermissions" && (permissions.files !== request.permissions.files || permissions.commands !== request.permissions.commands)) throw new Error("automation permission receipt differs");
  } else if (request.action === "cancelTeamTask") {
    keys(result, ["schemaVersion", "action", "chatId", "triggerMessageId", "status"]);
    if (result.chatId !== request.chatId || result.triggerMessageId !== request.triggerMessageId || !["stopping", "stopped"].includes(String(result.status))) throw new Error("team cancellation target mismatch");
  } else if (request.action === "inspectGitTrust" || request.action === "confirmGitTrust") {
    keys(result, ["schemaVersion", "action", "inspection"]);
    const inspection = validateGitTrustInspection(result.inspection);
    if (request.action === "confirmGitTrust" && (!inspection.trusted ||
      (Object.keys(request.identity) as (keyof GitTrustIdentity)[]).some(key => inspection.identity[key] !== request.identity[key]))) throw new Error("Git trust confirmation differs from inspection");
  } else if (request.action === "revokeGitTrust") {
    keys(result, ["schemaVersion", "action", "revoked"]);
    if (result.revoked !== true) throw new Error("Git trust revocation was not confirmed");
  } else if (request.action === "getProviderConnections" || request.action === "setProviderConnection") {
    keys(result,["schemaVersion","action","connections"]);
    const connections=object(result.connections); keys(connections,["codex","claude-code"]);
    if (typeof connections.codex !== "boolean" || typeof connections["claude-code"] !== "boolean") throw new Error("invalid provider connections");
    if (request.action === "setProviderConnection" && connections[request.runtime] !== request.connected) throw new Error("connection result mismatch");
  } else if (request.action === "getAppRoleDefaults" || request.action === "updateAppRoleDefaults") {
    keys(result, ["schemaVersion", "action", "settings"]);
    const settings = object(result.settings);
    keys(settings, ["schemaVersion", "revision", "roles", "updatedAt"]);
    validateProjectRoleSettingsV1({...settings, projectId: "app_defaults"});
    if (request.action === "updateAppRoleDefaults" && (settings.revision !== request.expectedRevision + 1 || !request.roles.every((role, index) => (["roleId", "runtime", "modelSelector", "reasoningEffort", "permission"] as const).every(key => role[key] === (settings.roles as readonly RoleProfileV1[])[index]?.[key])))) throw new Error("app defaults result differs from request");
  } else if (request.action === "inspectChatDiscard") {
    keys(result, ["schemaVersion", "action", "inspection"]);
    const inspection = validateChatDiscardInspection(result.inspection);
    for (const key of ["chatId", "taskId", "projectId", "worktreeId", "workspaceIdentity"] as const) if (inspection[key] !== request[key]) throw new Error("discard inspection address mismatch");
  } else if (request.action === "discardChatWorkspace") {
    keys(result, ["schemaVersion", "action", "chat"]);
    const chat = validateChatRecordV1(result.chat);
    for (const key of ["chatId", "taskId", "projectId", "worktreeId", "workspaceIdentity"] as const) if (chat[key] !== request.inspection[key]) throw new Error("discard result address mismatch");
    if (chat.state !== "discarded") throw new Error("discard result is not terminal");
  } else if (request.action === "applicationCapabilities") {
    keys(result, ["schemaVersion", "action", "enabled"]);
    if (typeof result.enabled !== "boolean") throw new Error("invalid application capability");
  } else if (request.action === "inspectApplicationRetirement") {
    keys(result, ["schemaVersion", "action", "projectId", "enabled", "inspection"]);
    const inspection = object(result.inspection); keys(inspection, ["applicationId", "fingerprint", "state"]);
    if (result.projectId !== request.projectId || inspection.applicationId !== request.applicationId || typeof result.enabled !== "boolean" || !["applied", "restored", "abandoned"].includes(String(inspection.state))) throw new Error("invalid retirement inspection");
    validateProjectCatalogRequest({action:"retireApplication",projectId:request.projectId,applicationId:inspection.applicationId,fingerprint:inspection.fingerprint});
  } else if (request.action === "retireApplication") {
    keys(result, ["schemaVersion", "action", "projectId", "applicationId"]);
    if (result.projectId !== request.projectId || result.applicationId !== request.applicationId) throw new Error("retirement target mismatch");
  } else if (request.action === "backupStorageUsage") {
    keys(result, ["schemaVersion", "action", "usage"]);
    const usage = object(result.usage);
    keys(usage, ["usedBytes", "limitBytes", "remainingBytes"]);
    for (const key of ["usedBytes", "limitBytes", "remainingBytes"]) {
      if (!Number.isSafeInteger(usage[key]) || (usage[key] as number) < 0) throw new Error("invalid backup storage amount");
    }
    backupLimit(usage.limitBytes);
    if (usage.remainingBytes !== Math.max(0, (usage.limitBytes as number) - (usage.usedBytes as number))) throw new Error("inconsistent backup storage budget");
  } else if (request.action === "setBackupStorageLimit") {
    keys(result, ["schemaVersion", "action", "limitBytes"]);
    if (result.limitBytes !== request.limitBytes) throw new Error("backup storage limit response mismatch");
  } else if (request.action === "backupRetention") {
    keys(result, ["schemaVersion", "action", "projectId", "entries"]);
    if (result.projectId !== request.projectId || !Array.isArray(result.entries) || result.entries.length > 1000) throw new Error("invalid backup retention list");
    const seen = new Set<string>();
    for (const raw of result.entries) {
      const entry = object(raw); keys(entry, ["reviewId", "state", "retainedFor", "cleanupState"]); id(entry.reviewId);
      if (!["none", "pending", "deleted"].includes(String(entry.cleanupState)) || (entry.cleanupState !== "none" && (entry.state === "preparing" || entry.retainedFor === "currentApplication"))) throw new Error("invalid backup cleanup state");
      if (seen.has(entry.reviewId as string) || !["preparing", "ready", "abandoned"].includes(String(entry.state)) || !["currentApplication", "applicationHistory", "unfinishedPreparation", "preparedBackup", "abandonedPreparation"].includes(String(entry.retainedFor))) throw new Error("invalid backup retention entry");
      seen.add(entry.reviewId as string);
    }
  } else if (request.action === "backupPreparations") {
    keys(result, ["schemaVersion", "action", "projectId", "entries"]);
    if (result.projectId !== request.projectId || !Array.isArray(result.entries) || result.entries.length > 1000) throw new Error("invalid backup preparation list");
    const seen = new Set<string>();
    for (const raw of result.entries) {
      const entry = object(raw); keys(entry, ["reviewId", "fingerprint"]);
      validateProjectCatalogRequest({action:"abandonBackupPreparation",projectId:request.projectId,...entry});
      if (seen.has(entry.reviewId as string)) throw new Error("duplicate preparation");
      seen.add(entry.reviewId as string);
    }
  } else if (request.action === "inspectBackupCleanup") {
    keys(result, ["schemaVersion", "action", "projectId", "enabled", "inspection"]);
    const inspection = object(result.inspection);
    keys(inspection, ["reviewId", "fingerprint", "filePresent", "state"]);
    if (result.projectId !== request.projectId || inspection.reviewId !== request.reviewId || typeof result.enabled !== "boolean" || typeof inspection.filePresent !== "boolean" || !["available", "pending", "deleted"].includes(String(inspection.state)) || (inspection.state === "deleted" && inspection.filePresent)) throw new Error("invalid cleanup inspection");
    validateProjectCatalogRequest({action:"cleanupBackup",projectId:request.projectId,reviewId:inspection.reviewId,fingerprint:inspection.fingerprint});
  } else if (request.action === "cleanupBackup" || request.action === "abandonBackupPreparation") {
    keys(result, ["schemaVersion", "action", "projectId", "reviewId"]);
    if (result.projectId !== request.projectId || result.reviewId !== request.reviewId) throw new Error("backup preparation response mismatch");
  } else if (request.action === "startApplication" || request.action === "restoreApplication") {
    keys(result, request.action === "startApplication" ? ["schemaVersion", "action", "projectId", "chatId", "applicationId"] : ["schemaVersion", "action", "projectId", "applicationId"]);
    validateProjectCatalogRequest({action:"cancelApplication",projectId:result.projectId,applicationId:result.applicationId});
    if (result.projectId !== request.projectId || (request.action === "startApplication" ? result.chatId !== request.chatId : result.applicationId !== request.applicationId)) throw new Error("application start target mismatch");
  } else if (request.action === "restorationPreview") {
    keys(result, ["schemaVersion", "action", "projectId", "applicationId", "state", "page"]);
    if (result.projectId !== request.projectId || result.applicationId !== request.applicationId || !["prepared", "applying", "applied"].includes(String(result.state))) throw new Error("restoration target mismatch");
    validateProjectCatalogResult({schemaVersion:1,action:"applicationPreviewPage",projectId:result.projectId,chatId:null,page:result.page}, {action:"applicationPreviewPage",projectId:request.projectId,chatId:null,offset:request.offset,fingerprint:request.fingerprint});
  } else if (request.action === "cancelApplication") {
    keys(result, ["schemaVersion", "action", "projectId", "applicationId"]);
    if (result.projectId !== request.projectId || result.applicationId !== request.applicationId) throw new Error("application cancellation mismatch");
  } else if (request.action === "openApplicationConfirmation") {
    keys(result, ["schemaVersion", "action", "projectId", "chatId", "confirmationId", "worktreeId", "total", "expiresInSeconds"]);
    if (result.projectId !== request.projectId || result.chatId !== request.chatId || result.expiresInSeconds !== 300 || !Number.isSafeInteger(result.total) || (result.total as number) < 0 || (result.total as number) > 100000) throw new Error("invalid confirmation response");
    id(result.worktreeId);
    validateProjectCatalogRequest({action:"cancelApplicationConfirmation",confirmationId:result.confirmationId});
  } else if (request.action === "cancelApplicationConfirmation") {
    keys(result, ["schemaVersion", "action", "confirmationId"]);
    if (result.confirmationId !== request.confirmationId) throw new Error("confirmation cancellation mismatch");
  } else if (request.action === "applicationPreviewPage") {
    keys(result, ["schemaVersion", "action", "projectId", "chatId", "page"]);
    if (result.projectId !== request.projectId || result.chatId !== request.chatId) throw new Error("review page target mismatch");
    const page = object(result.page); keys(page, ["worktreeId", "total", "changes", "offset", "fingerprint"]);
    if (page.offset !== request.offset || !Number.isSafeInteger(page.total) || (page.total as number) < request.offset || (page.total as number) > 100000 || typeof page.fingerprint !== "string" || !/^[a-f0-9]{64}$/.test(page.fingerprint) || (request.fingerprint !== null && page.fingerprint !== request.fingerprint) || !Array.isArray(page.changes) || page.changes.length !== Math.min(100, (page.total as number) - request.offset)) throw new Error("review page mismatch");
    // Reuse the exact path/kind and bounded-list validator without treating a
    // later page as the first page of the complete change set.
    validateProjectCatalogResult({schemaVersion:1,action:"applicationPreview",projectId:request.projectId,preview:{worktreeId:page.worktreeId,total:page.changes.length,changes:page.changes}}, {action:"applicationPreview",projectId:request.projectId});
  } else if (request.action === "prepareApplicationWorkspace") {
    keys(result, ["schemaVersion", "action", "projectId", "worktreeId"]);
    if (id(result.projectId) !== request.projectId) throw new Error("prepared application project mismatch");
    id(result.worktreeId);
  } else if (request.action === "applicationPreview" || request.action === "chatApplicationPreview") {
    keys(result, request.action === "chatApplicationPreview" ? ["schemaVersion", "action", "projectId", "chatId", "preview"] : ["schemaVersion", "action", "projectId", "preview"]);
    if (request.action === "chatApplicationPreview" && id(result.chatId) !== request.chatId) throw new Error("preview chat mismatch");
    if (id(result.projectId) !== request.projectId) throw new Error("preview project mismatch");
    const preview = object(result.preview);
    keys(preview, ["worktreeId", "total", "changes"]); id(preview.worktreeId);
    if (!Number.isSafeInteger(preview.total) || (preview.total as number) < 0 || (preview.total as number) > 100000 || !Array.isArray(preview.changes) || preview.changes.length !== Math.min(100, preview.total as number)) throw new Error("invalid preview bounds");
    const seen = new Set<string>();
    for (const raw of preview.changes) {
      const change = object(raw); keys(change, ["path", "kind"]);
      if (typeof change.path !== "string" || !change.path || new TextEncoder().encode(change.path).byteLength > 4096 || change.path.includes("\0") || change.path.split("/").some(part => !part || part === "." || part === "..") || seen.has(change.path) || !["added", "modified", "deleted"].includes(String(change.kind))) throw new Error("invalid preview path");
      seen.add(change.path);
    }
  } else if (request.action === "applicationHistory") {
    keys(result, ["schemaVersion", "action", "projectId", "entries"]);
    if (id(result.projectId) !== request.projectId || !Array.isArray(result.entries) || result.entries.length > 1000) throw new Error("invalid application history");
    const seen = new Set<string>();
    for (const raw of result.entries) {
      const entry = object(raw);
      keys(entry, ["applicationId", "state", "total"]);
      if (typeof entry.applicationId !== "string" || !/^application_[a-f0-9]{32}$/.test(entry.applicationId) || seen.has(entry.applicationId)) throw new Error("invalid historical ID");
      seen.add(entry.applicationId);
      if (!["applied", "restored", "abandoned"].includes(String(entry.state)) || !Number.isSafeInteger(entry.total) || (entry.total as number) < 0 || (entry.total as number) > 100000) throw new Error("invalid historical summary");
    }
  } else if (request.action === "applicationStatus" || request.action === "chatApplicationStatus") {
    keys(result, request.action === "chatApplicationStatus" ? ["schemaVersion","action","chatId","status"] : ["schemaVersion","action","status"]);
    if (request.action === "chatApplicationStatus" && id(result.chatId) !== request.chatId) throw new Error("application status chat mismatch");
    const status = object(result.status);
    keys(status,["projectId","applicationId","state","currentIndex","total","running","failure"]);
    if (status.failure !== null) {
      const failure = object(status.failure); keys(failure, ["code", "detail"]);
      if (status.running || status.state === "none" || typeof failure.code !== "string" || !/^E_[A-Z_]{1,80}$/.test(failure.code) || typeof failure.detail !== "string" || [...failure.detail].length > 2048) throw new Error("invalid application failure");
    }
    if (typeof status.running !== "boolean" || (status.running && !["prepared", "applying", "applied"].includes(String(status.state)))) throw new Error("invalid application execution state");
    if (id(status.projectId) !== request.projectId || !["none","prepared","applying","applied","restored","abandoned"].includes(String(status.state))) throw new Error("invalid application status");
    if (!Number.isSafeInteger(status.total) || (status.total as number) < 0 || (status.total as number) > 100000) throw new Error("invalid application count");
    if (status.state === "none") {
      if (status.applicationId !== null || status.currentIndex !== null || status.total !== 0) throw new Error("invalid empty application");
    } else if (typeof status.applicationId !== "string" || !/^application_[a-f0-9]{32}$/.test(status.applicationId)) throw new Error("invalid application ID");
    if (status.currentIndex !== null && (!Number.isSafeInteger(status.currentIndex) || (status.currentIndex as number) < 0 || (status.currentIndex as number) >= (status.total as number))) throw new Error("invalid application cursor");
  } else if (request.action === "inspectPreparation" || request.action === "abandonPreparation") {
    keys(result, ["schemaVersion", "action", "inspection"]);
    const inspection = validatePreparationInspection(result.inspection);
    if (request.action === "inspectPreparation") {
      if (inspection.projectId !== request.projectId) throw new Error("preparation project mismatch");
    } else if (Object.keys(inspection).some(key => inspection[key as keyof PreparationInspection] !== request.inspection[key as keyof PreparationInspection])) throw new Error("preparation response mismatch");
  } else if (request.action === "removeChat") {
    keys(result, ["schemaVersion", "action", "projectId", "chatId"]);
    if (id(result.projectId) !== request.projectId || id(result.chatId) !== request.chatId) throw new Error("catalog removal identity mismatch");
  } else if (request.action === "releaseView" || request.action === "removeProject" || request.action === "restoreChats") {
    keys(result, ["schemaVersion", "action", "projectId"]);
    if (id(result.projectId) !== request.projectId) throw new Error("catalog release identity mismatch");
  } else if (request.action === "reopen") {
    keys(result, ["schemaVersion", "action", "project"]);
    const project = validateProjectIdentityV1(result.project);
    if (project.projectId !== request.projectId) throw new Error("catalog selection mismatch");
  } else {
    keys(result, ["schemaVersion", "action", "projects", "nextCursor"]);
    if (!Array.isArray(result.projects) || result.projects.length > 50) throw new Error("catalog page exceeds limit");
    let previous = request.cursor ?? "";
    for (const raw of result.projects) {
      const entry = object(raw); keys(entry, ["projectId", "canonicalPath"]);
      const projectId = id(entry.projectId);
      if (projectId <= previous) throw new Error("catalog order or cursor mismatch");
      validateProjectIdentityV1({ schemaVersion: 1, projectId, selectedPath: entry.canonicalPath, canonicalGitRoot: entry.canonicalPath });
      previous = projectId;
    }
    if (result.nextCursor !== null && (!result.projects.length || id(result.nextCursor) !== previous)) throw new Error("catalog continuation mismatch");
  }
  return result as unknown as ProjectCatalogResult;
}
