import { validateProjectIdentityV1, type ProjectIdentityV1 } from "./project-v1.ts";

export const PROJECT_CATALOG_METHOD = "project.catalog";
export const PROJECT_CATALOG_CHANNEL = "m5:project:catalog";
export interface PreparationInspection { projectId: string; worktreeId: string; workspaceIdentity: string; lockId: string; projectRevision: number; lockRevision: number }
export interface ApplicationStatus { projectId: string; applicationId: string | null; state: "none" | "prepared" | "applying" | "applied" | "restored" | "abandoned"; currentIndex: number | null; total: number }
export interface ApplicationHistoryEntry { applicationId: string; state: "applied" | "restored" | "abandoned"; total: number }
export type ProjectCatalogRequest = { action: "list"; cursor: string | null } | { action: "reopen"; projectId: string } | { action: "releaseView"; projectId: string }
  | { action: "inspectPreparation"; projectId: string }
  | { action: "applicationStatus"; projectId: string }
  | { action: "applicationHistory"; projectId: string }
  | { action: "abandonPreparation"; inspection: PreparationInspection };
export interface ProjectCatalogEntry { projectId: string; canonicalPath: string }
export type ProjectCatalogResult =
  | { schemaVersion: 1; action: "applicationHistory"; projectId: string; entries: ApplicationHistoryEntry[] }
  | { schemaVersion: 1; action: "applicationStatus"; status: ApplicationStatus }
  | { schemaVersion: 1; action: "inspectPreparation" | "abandonPreparation"; inspection: PreparationInspection }
  | { schemaVersion: 1; action: "list"; projects: ProjectCatalogEntry[]; nextCursor: string | null }
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
export function validatePreparationInspection(value: unknown): PreparationInspection {
  const inspection = object(value);
  keys(inspection, ["projectId", "worktreeId", "workspaceIdentity", "lockId", "projectRevision", "lockRevision"]);
  for (const key of ["projectId", "worktreeId", "workspaceIdentity", "lockId"]) id(inspection[key]);
  if (inspection.lockId !== `lock_${inspection.worktreeId}`) throw new Error("preparation lock mismatch");
  for (const key of ["projectRevision", "lockRevision"]) if (!Number.isSafeInteger(inspection[key]) || (inspection[key] as number) < 1) throw new Error("invalid preparation revision");
  return inspection as unknown as PreparationInspection;
}
export function validateProjectCatalogRequest(value: unknown): ProjectCatalogRequest {
  const request = object(value);
  if (request.action === "list") {
    keys(request, ["action", "cursor"]);
    if (request.cursor !== null) id(request.cursor);
  } else if (request.action === "reopen" || request.action === "releaseView" || request.action === "inspectPreparation" || request.action === "applicationStatus" || request.action === "applicationHistory") {
    keys(request, ["action", "projectId"]); id(request.projectId);
  } else if (request.action === "abandonPreparation") {
    keys(request, ["action", "inspection"]); validatePreparationInspection(request.inspection);
  } else throw new Error("invalid project catalog action");
  return request as ProjectCatalogRequest;
}
export function validateProjectCatalogResult(value: unknown, input: ProjectCatalogRequest): ProjectCatalogResult {
  const request = validateProjectCatalogRequest(input);
  const result = object(value);
  if (result.schemaVersion !== 1 || result.action !== request.action || new TextEncoder().encode(JSON.stringify(value)).byteLength > 768 * 1024) throw new Error("invalid catalog response envelope");
  if (request.action === "applicationHistory") {
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
  } else if (request.action === "applicationStatus") {
    keys(result,["schemaVersion","action","status"]);
    const status = object(result.status);
    keys(status,["projectId","applicationId","state","currentIndex","total"]);
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
  } else if (request.action === "releaseView") {
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
