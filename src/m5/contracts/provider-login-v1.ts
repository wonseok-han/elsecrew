export type LoginState = "idle" | "starting" | "waiting" | "connected" | "canceled" | "failed" | "unsupported";
export type LoginStatus = { state: LoginState; attemptId: string | null; authUrl: string | null; error: string | null };
export type LoginRequest = { action: "status" } | { action: "start" | "cancel"; attemptId: string };
export const LOGIN_CHANNEL = "m5:login:v1";
export function validateLoginRequest(input: unknown): LoginRequest {
  if (!input || typeof input !== "object" || Array.isArray(input)) throw new Error("invalid login request");
  const v = input as Record<string,unknown>;
  if (v.action === "status" && Object.keys(v).length === 1) return {action:"status"};
  if ((v.action === "start" || v.action === "cancel") && Object.keys(v).length === 2 && typeof v.attemptId === "string" && /^[a-zA-Z0-9-]{1,80}$/.test(v.attemptId)) return {action:v.action,attemptId:v.attemptId};
  throw new Error("invalid login request");
}
export function validateLoginStatus(input: unknown): LoginStatus {
  if (!input || typeof input !== "object" || Array.isArray(input)) throw new Error("invalid login status");
  const v = input as Record<string,unknown>;
  if (Object.keys(v).sort().join() !== "attemptId,authUrl,error,state" || !["idle","starting","waiting","connected","canceled","failed","unsupported"].includes(String(v.state))) throw new Error("invalid login status");
  if (v.attemptId !== null && (typeof v.attemptId !== "string" || !/^[a-zA-Z0-9-]{1,80}$/.test(v.attemptId))) throw new Error("invalid attempt");
  if (v.error !== null && (typeof v.error !== "string" || !/^[a-zA-Z]{1,80}$/.test(v.error))) throw new Error("invalid login error");
  if (v.authUrl !== null) {
    if (typeof v.authUrl !== "string" || v.authUrl.length > 16384 || /[\u0000-\u0020\\]/.test(v.authUrl)) throw new Error("invalid login URL");
    const url = new URL(v.authUrl);
    if (url.protocol !== "https:" || !["auth.openai.com","chatgpt.com","auth.chatgpt.com"].includes(url.hostname) || url.username || url.password || url.port || v.state !== "waiting") throw new Error("invalid login URL");
  }
  return v as LoginStatus;
}
