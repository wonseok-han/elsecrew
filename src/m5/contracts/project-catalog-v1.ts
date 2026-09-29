import { validateProjectIdentityV1, type ProjectIdentityV1 } from "./project-v1.ts";

export const PROJECT_CATALOG_METHOD = "project.catalog";
export const PROJECT_CATALOG_CHANNEL = "m5:project:catalog";
export type ProjectCatalogRequest = { action: "list"; cursor: string | null } | { action: "reopen"; projectId: string };
export interface ProjectCatalogEntry { projectId: string; canonicalPath: string }
export type ProjectCatalogResult =
  | { schemaVersion: 1; action: "list"; projects: ProjectCatalogEntry[]; nextCursor: string | null }
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
export function validateProjectCatalogRequest(value: unknown): ProjectCatalogRequest {
  const request = object(value);
  if (request.action === "list") {
    keys(request, ["action", "cursor"]);
    if (request.cursor !== null) id(request.cursor);
  } else if (request.action === "reopen") {
    keys(request, ["action", "projectId"]); id(request.projectId);
  } else throw new Error("invalid project catalog action");
  return request as ProjectCatalogRequest;
}
export function validateProjectCatalogResult(value: unknown, input: ProjectCatalogRequest): ProjectCatalogResult {
  const request = validateProjectCatalogRequest(input);
  const result = object(value);
  if (result.schemaVersion !== 1 || result.action !== request.action || new TextEncoder().encode(JSON.stringify(value)).byteLength > 768 * 1024) throw new Error("invalid catalog response envelope");
  if (request.action === "reopen") {
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
