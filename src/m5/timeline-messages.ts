/**
 * Which saved messages the conversation shows. Presentation only: nothing is
 * removed from the record, and every message stays in the recovery history.
 */
interface TimelineMessage { kind: string; text: string; roleId: string | null }

/** The engine's step-by-step filler ("still working"); the run cards already say it. */
const FILLER_STATUS = new Set(["팀 작업을 이어가고 있어요."]);

export function timelineMessages<T extends TimelineMessage>(messages: readonly T[], goal: string | null): T[] {
  const title = goal?.trim() ?? "";
  let firstUser = true;
  return messages.filter(message => {
    if (message.kind === "status" && FILLER_STATUS.has(message.text.trim())) return false;
    if (message.kind === "user") {
      // The task's goal is its title; its first message would repeat it.
      const repeat = firstUser && title !== "" && message.text.trim() === title;
      firstUser = false;
      if (repeat) return false;
    }
    return true;
  });
}
