import type { ChatStateV1 } from "./contracts/run-recovery-v1.ts";

export interface ProjectGuidePreference { version: 1; chatId: string | null; teamReviewed: boolean; dismissed: boolean }
export const emptyProjectGuide = (): ProjectGuidePreference => ({ version: 1, chatId: null, teamReviewed: false, dismissed: false });
export function parseProjectGuide(raw: string | null): ProjectGuidePreference {
  if (!raw || raw.length > 1024) return emptyProjectGuide();
  try {
    const value = JSON.parse(raw);
    if (!value || Object.keys(value).sort().join() !== "chatId,dismissed,teamReviewed,version" || value.version !== 1
      || typeof value.teamReviewed !== "boolean" || typeof value.dismissed !== "boolean"
      || (value.chatId !== null && (typeof value.chatId !== "string" || !/^[a-zA-Z0-9_-]{1,128}$/.test(value.chatId)))) return emptyProjectGuide();
    return value;
  } catch { return emptyProjectGuide(); }
}
export function projectGuideKey(projectId: string): string { return `hello-agent.m5.guide.v1:${encodeURIComponent(projectId)}`; }
export function projectGuideStep(preference: ProjectGuidePreference, activeChatId: string | null, state: ChatStateV1 | null): "team" | "goal" | "resume" | "observe" | "review" | "attention" | "recovery" {
  if (preference.chatId && preference.chatId !== activeChatId) return "resume";
  if (activeChatId) {
    if (state === "recoveryRequired") return "recovery";
    if (state === "needsAttention") return "attention";
    if (state === "reviewReady" || state === "applying" || state === "completed" || state === "discarded") return "review";
    return "observe";
  }
  return preference.teamReviewed ? "goal" : "team";
}
