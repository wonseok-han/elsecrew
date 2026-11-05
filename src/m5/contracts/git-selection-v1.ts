export const GIT_SELECTION_CHANNEL = "m5:git-selection:v1";
export type GitSelectionAction = "inspect" | "choose" | "forget";
export type GitSelection = {
  state: "unsupported" | "unselected" | "selected" | "unavailable";
  path: string | null;
  executionEnabled: false;
};
export function validateGitSelectionAction(value: unknown): GitSelectionAction {
  if (value !== "inspect" && value !== "choose" && value !== "forget") throw new Error("invalid Git selection action");
  return value;
}
export function validateGitSelection(value: unknown): GitSelection {
  if (!value || typeof value !== "object" || Array.isArray(value)) throw new Error("invalid Git selection");
  const v = value as Record<string, unknown>;
  if (Object.keys(v).sort().join() !== "executionEnabled,path,state" || v.executionEnabled !== false
    || !["unsupported", "unselected", "selected", "unavailable"].includes(String(v.state))
    || (v.state === "selected" ? typeof v.path !== "string" || v.path.length === 0 || v.path.length > 32767 || /[\u0000-\u001f]/.test(v.path) : v.path !== null)) {
    throw new Error("invalid Git selection");
  }
  return v as GitSelection;
}
