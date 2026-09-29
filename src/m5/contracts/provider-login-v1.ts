export type LoginState = "idle" | "starting" | "waiting" | "verifying" | "connected" | "canceled" | "failed" | "unsupported";
export type LoginStatus = { state: LoginState; attemptId: string | null; authUrl: string | null; error: string | null };
export type LoginProvider = "codex" | "claude";
export type LoginRequest = (({ action: "status" } | { action: "start" | "check" | "cancel" | "recover"; attemptId: string }) & {provider?: LoginProvider}) | {action:"submitCode"; provider:"claude"; attemptId:string; code:string};
export const LOGIN_CHANNEL = "m5:login:v1";
export function validateLoginRequest(input: unknown): LoginRequest {
  if (!input || typeof input !== "object" || Array.isArray(input)) throw new Error("invalid login request");
  const v = input as Record<string,unknown>;
  const hasProvider = Object.prototype.hasOwnProperty.call(v,"provider");
  if (hasProvider && v.provider !== "codex" && v.provider !== "claude") throw new Error("invalid login provider");
  const provider = hasProvider ? {provider: v.provider as LoginProvider} : {};
  const size = Object.keys(v).length - Number(hasProvider);
  if (v.action === "submitCode" && v.provider === "claude" && size === 3 && typeof v.attemptId === "string" && /^[a-zA-Z0-9-]{1,80}$/.test(v.attemptId) && typeof v.code === "string" && /^[a-zA-Z0-9_#.=~-]{1,4096}$/.test(v.code)) return {action:"submitCode",provider:"claude",attemptId:v.attemptId,code:v.code};
  if (v.action === "status" && size === 1) return {action:"status",...provider};
  if ((v.action === "start" || v.action === "check" || v.action === "cancel" || v.action === "recover") && size === 2 && typeof v.attemptId === "string" && /^[a-zA-Z0-9-]{1,80}$/.test(v.attemptId)) return {action:v.action,attemptId:v.attemptId,...provider};
  throw new Error("invalid login request");
}
export function validateLoginStatus(input: unknown, provider: LoginProvider = "codex"): LoginStatus {
  if (!input || typeof input !== "object" || Array.isArray(input)) throw new Error("invalid login status");
  const v = input as Record<string,unknown>;
  if (Object.keys(v).sort().join() !== "attemptId,authUrl,error,state" || !["idle","starting","waiting","verifying","connected","canceled","failed","unsupported"].includes(String(v.state))) throw new Error("invalid login status");
  if (v.attemptId !== null && (typeof v.attemptId !== "string" || !/^[a-zA-Z0-9-]{1,80}$/.test(v.attemptId))) throw new Error("invalid attempt");
  if (v.error !== null && (typeof v.error !== "string" || !/^[a-zA-Z]{1,80}$/.test(v.error))) throw new Error("invalid login error");
  if (v.authUrl !== null) {
    if (typeof v.authUrl !== "string" || v.authUrl.length > 16384 || /[\u0000-\u0020\\]/.test(v.authUrl)) throw new Error("invalid login URL");
    const url = new URL(v.authUrl);
    const hosts = provider === "claude" ? ["claude.ai","claude.com"] : ["auth.openai.com","chatgpt.com","auth.chatgpt.com"];
    const claudePath = url.hostname === "claude.com" ? "/cai/oauth/authorize" : "/oauth/authorize";
    if (url.protocol !== "https:" || !hosts.includes(url.hostname) || url.username || url.password || url.port || v.state !== "waiting" || (provider === "claude" && url.pathname !== claudePath)) throw new Error("invalid login URL");
  }
  return v as LoginStatus;
}
