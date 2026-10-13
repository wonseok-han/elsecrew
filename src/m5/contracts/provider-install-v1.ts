export const INSTALL_CHANNEL = "m5:install:v1";
export type InstallProvider = "codex" | "claude";
export type InstallRequest = { action: "status" | "dismiss" }
  | { action: "prepare"; provider: InstallProvider }
  | { action: "start"; provider: InstallProvider; planId: string }
  | { action: "cancel"; operationId: string };
export type InstallPlan = {
  planId: string; provider: InstallProvider; action: "installOrUpdate"; expiresInSeconds: 180;
  writeDirectories: string[]; replacesExistingCommands: true; shellSettingsWritable: false; startsLogin: false;
};
export type InstallState = "idle" | "awaitingConsent" | "starting" | "downloading" | "installing"
  | "verifying" | "stopping" | "installed" | "canceled" | "failed" | "recoveryRequired" | "unsupported";
export type InstallStatus = { state: InstallState; operationId: string | null; provider: InstallProvider | null;
  plan: InstallPlan | null; error: string | null; cancelRequested: boolean };

function object(value: unknown): Record<string, unknown> {
  if (!value || typeof value !== "object" || Array.isArray(value)) throw new Error("invalid installation object");
  return value as Record<string, unknown>;
}
function keys(value: Record<string, unknown>, expected: string[]): void {
  if (Object.keys(value).sort().join() !== expected.sort().join()) throw new Error("invalid installation fields");
}
function id(value: unknown): value is string {
  return typeof value === "string" && /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/.test(value)
    && value !== "00000000-0000-0000-0000-000000000000";
}
function provider(value: unknown): value is InstallProvider { return value === "codex" || value === "claude"; }
export function validateInstallRequest(input: unknown): InstallRequest {
  const v = object(input);
  if (v.action === "status" || v.action === "dismiss") {
    keys(v, ["action"]); return { action: v.action };
  }
  if (v.action === "prepare" && provider(v.provider)) {
    keys(v, ["action", "provider"]); return { action: v.action, provider: v.provider };
  }
  if (v.action === "start" && provider(v.provider) && id(v.planId)) {
    keys(v, ["action", "provider", "planId"]); return { action: v.action, provider: v.provider, planId: v.planId };
  }
  if (v.action === "cancel" && id(v.operationId)) {
    keys(v, ["action", "operationId"]); return { action: v.action, operationId: v.operationId };
  }
  throw new Error("invalid installation request");
}
function plan(input: unknown): InstallPlan {
  const v = object(input);
  keys(v, ["planId", "provider", "action", "expiresInSeconds", "writeDirectories", "replacesExistingCommands", "shellSettingsWritable", "startsLogin"]);
  if (!id(v.planId) || !provider(v.provider) || v.action !== "installOrUpdate" || v.expiresInSeconds !== 180
    || v.replacesExistingCommands !== true || v.shellSettingsWritable !== false || v.startsLogin !== false
    || !Array.isArray(v.writeDirectories) || v.writeDirectories.length !== (v.provider === "codex" ? 2 : 5)
    || !v.writeDirectories.every(path => typeof path === "string" && path.startsWith("/") && path.length <= 4200
      && !/[\u0000-\u001f\u007f-\u009f:]/.test(path))) throw new Error("invalid installation plan");
  const suffixes = v.provider === "codex" ? ["/.local/bin", "/.codex/packages/standalone"]
    : ["/.local/bin", "/.claude/downloads", "/.local/share/claude", "/.local/state/claude", "/.cache/claude"];
  const home = (v.writeDirectories[0] as string).slice(0, -suffixes[0].length);
  if (!home.startsWith("/") || home.split("/").slice(1).some(part => !part || part === "." || part === "..")
    || !v.writeDirectories.every((path, index) => path === home + suffixes[index])) throw new Error("inconsistent installation destination");
  return v as InstallPlan;
}
export function validateInstallStatus(input: unknown): InstallStatus {
  const v = object(input);
  keys(v, ["state", "operationId", "provider", "plan", "error", "cancelRequested"]);
  if (!["idle", "awaitingConsent", "starting", "downloading", "installing", "verifying", "stopping", "installed", "canceled", "failed", "recoveryRequired", "unsupported"].includes(String(v.state))
    || (v.operationId !== null && !id(v.operationId)) || (v.provider !== null && !provider(v.provider))
    || typeof v.cancelRequested !== "boolean" || (v.error !== null && (typeof v.error !== "string" || !/^[a-zA-Z]{1,80}$/.test(v.error)))) throw new Error("invalid installation status");
  if (v.state === "awaitingConsent") {
    const p = plan(v.plan);
    if (p.provider !== v.provider || v.operationId !== null || v.cancelRequested || v.error !== null) throw new Error("invalid installation consent state");
  } else if (v.plan !== null) throw new Error("unexpected installation plan");
  if (["starting", "downloading", "installing", "verifying", "stopping", "installed", "canceled"].includes(String(v.state))
    && (!id(v.operationId) || !provider(v.provider))) throw new Error("missing installation operation");
  if (["idle", "unsupported"].includes(String(v.state)) && (v.provider !== null || v.operationId !== null || v.error !== null || v.cancelRequested)) throw new Error("invalid inactive installation state");
  if (v.cancelRequested && !["starting", "downloading", "installing", "verifying", "stopping", "recoveryRequired"].includes(String(v.state))) throw new Error("unexpected installation cancel");
  return v as InstallStatus;
}
