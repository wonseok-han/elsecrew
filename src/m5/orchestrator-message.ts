/** Legacy V1/V2 team. V3 callers pass the chat's own roleIds; the sidecar enforces membership. */
const LEGACY_TEAM = ["coordinator", "planning", "design", "coding", "review"] as const;
const isWorkerRole = (role: unknown, team: readonly string[]): role is string => typeof role === "string" && role !== "coordinator" && team.includes(role);

/** Presentation only. Execution decisions are validated and scheduled by the sidecar. */
export function orchestratorMessage(text: string, role: string | null, team: readonly string[] = LEGACY_TEAM): string {
  if (role !== "coordinator") return text;
  try {
    const value: unknown = JSON.parse(text);
    if (!value || typeof value !== "object" || Array.isArray(value)) return text;
    const item = value as Record<string, unknown>;
    if (item.action === "final" && Object.keys(item).sort().join() === "action,answer"
        && typeof item.answer === "string" && item.answer.trim()) return item.answer;
    if (item.action === "delegate" && Object.keys(item).sort().join() === "action,instruction,role,summary"
        && isWorkerRole(item.role, team)
        && typeof item.summary === "string" && item.summary.trim()
        && typeof item.instruction === "string" && item.instruction.trim()) return item.summary;
  } catch { /* Ordinary text and incomplete streamed output remain readable. */ }
  return text;
}

/** Explicit delegated task only; never private model reasoning or executable authority. */
export function orchestratorDelegation(text: string, role: string | null, team: readonly string[] = LEGACY_TEAM): { role: string; instruction: string } | null {
  if (role !== "coordinator") return null;
  try {
    const item = JSON.parse(text);
    if (item && typeof item === "object" && !Array.isArray(item)
        && Object.keys(item).sort().join() === "action,instruction,role,summary"
        && item.action === "delegate" && isWorkerRole(item.role, team)
        && typeof item.summary === "string" && item.summary.trim()
        && typeof item.instruction === "string" && item.instruction.trim()) {
      return { role: item.role, instruction: item.instruction };
    }
  } catch { /* Incomplete/plain text remains ordinary message content. */ }
  return null;
}
