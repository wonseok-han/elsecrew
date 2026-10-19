import { ko, type MessageKey } from "../locales/ko.ts";
import { en } from "../locales/en.ts";
import type { RoleProfileV1 } from "./contracts/run-recovery-v1.ts";

export type ChatNoticeKey = Extract<MessageKey, `m5.chatNotice.${string}` | `m5.chatError.${string}` | "m5.agentNotice.checking" | "m5.agentNotice.ready" | "m5.agentNotice.blocked">;
export type ChatNoticeContext = { roleId: RoleProfileV1["roleId"]; runtime: RoleProfileV1["runtime"]; reason?: string | null };
type Notice = { notice: string | null; noticeKey?: ChatNoticeKey; noticeContext?: ChatNoticeContext };
const causes = ["platformUnsupported", "notInstalled", "installationIncomplete", "unsupportedVersion", "notAuthenticated", "authExpired", "capabilityUnknown", "exactModelUnavailable"] as const;

export function formatChatNotice(value: Notice, lang: "ko" | "en"): string {
  if (!value.noticeKey) return value.notice ?? "";
  const dict = lang === "ko" ? ko : en;
  const context = value.noticeContext;
  const cause = causes.find(cause => cause === context?.reason) ?? "unknown";
  const params: Record<string, string> = {
    role: context ? dict[`m5.roles.${context.roleId}`] : "",
    runtime: context?.runtime === "codex" ? "Codex" : "Claude Code",
    cause: dict[`m5.agentNotice.cause.${cause}`],
  };
  return dict[value.noticeKey].replace(/\{(role|runtime|cause)\}/g, (_, key) => params[key]);
}

export function chatNotice(noticeKey: ChatNoticeKey, noticeContext?: ChatNoticeContext) {
  return { notice: formatChatNotice({ notice: null, noticeKey, noticeContext }, "ko"), noticeKey, noticeContext };
}

export function recoveryConfirmation(actual: boolean, lang: "ko" | "en"): string {
  return (lang === "ko" ? ko : en)[actual ? "m5.agentNotice.confirmAgent" : "m5.agentNotice.confirmFixture"];
}

export function agentContextConfirmation(total: number, lang: "ko" | "en"): string {
  return (lang === "ko" ? ko : en)["m5.chat.contextConfirmation"]
    .replace("{total}", String(total)).replace("{omitted}", String(total - 100));
}
