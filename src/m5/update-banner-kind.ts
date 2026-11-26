/** main's app update states (electron/app-update.ts). */
export type UpdateState = "unavailable" | "idle" | "current" | "checking" | "downloading" | "ready" | "restarting" | "error";
export type UpdateBannerKind = "downloading" | "ready" | "restarting" | "error";
const SHOWN: readonly UpdateState[] = ["downloading", "ready", "restarting", "error"];

/** Which in-app banner a state shows, or none (ADE U8). */
export function updateBannerKind(state: UpdateState): UpdateBannerKind | null {
  return SHOWN.includes(state) ? state as UpdateBannerKind : null;
}
