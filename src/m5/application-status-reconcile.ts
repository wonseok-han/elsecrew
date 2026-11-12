import { validateProjectCatalogResult, type ApplicationStatus, type ProjectCatalogRequest } from "./contracts/project-catalog-v1.ts";

type StatusQuery = Extract<ProjectCatalogRequest, { action: "applicationStatus" | "chatApplicationStatus" }>;

export function reconcileApplicationStatus(raw: unknown, query: StatusQuery, expectedId: string | null, previousId: string | null | undefined): ApplicationStatus {
  const result = validateProjectCatalogResult(raw, query);
  if (result.action !== "applicationStatus" && result.action !== "chatApplicationStatus") throw new Error("unexpected application status");
  const status = result.status;
  if (!status.applicationId || (expectedId
    ? status.applicationId !== expectedId
    : query.action !== "chatApplicationStatus" || previousId === undefined || status.applicationId === previousId)) {
    throw new Error("application attempt is not identified");
  }
  return status;
}
