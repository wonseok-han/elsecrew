/** main's app update states (electron/app-update.ts). */
export type UpdateState = "unavailable" | "idle" | "current" | "checking" | "downloading" | "ready" | "restarting" | "error";
export type UpdateBannerKind = "checking" | "current" | "downloading" | "ready" | "restarting" | "error";
/** Everything a check can lead to shows in the app (PR #16 review I4): the menu and settings no longer answer in native boxes. */
const SHOWN: readonly UpdateState[] = ["checking", "current", "downloading", "ready", "restarting", "error"];

/** Which in-app banner a state shows, or none (ADE U8). */
export function updateBannerKind(state: UpdateState): UpdateBannerKind | null {
  return SHOWN.includes(state) ? state as UpdateBannerKind : null;
}
