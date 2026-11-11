/** Presentation only. Execution decisions are validated and scheduled by the sidecar. */
export function orchestratorMessage(text: string, role: string | null): string {
  if (role !== "coordinator") return text;
  try {
    const value: unknown = JSON.parse(text);
    if (!value || typeof value !== "object" || Array.isArray(value)) return text;
    const item = value as Record<string, unknown>;
    if (item.action === "final" && Object.keys(item).sort().join() === "action,answer"
        && typeof item.answer === "string" && item.answer.trim()) return item.answer;
    if (item.action === "delegate" && Object.keys(item).sort().join() === "action,instruction,role,summary"
        && typeof item.role === "string" && ["planning", "design", "coding", "review"].includes(item.role)
        && typeof item.summary === "string" && item.summary.trim()
        && typeof item.instruction === "string" && item.instruction.trim()) return item.summary;
  } catch { /* Ordinary text and incomplete streamed output remain readable. */ }
  return text;
}
