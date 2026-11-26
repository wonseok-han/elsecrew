import type { TaskPhase } from "./task-status.ts";

/**
 * The task layout (ADE 1차-나): task list on the left, the task's story in
 * the centre, its results on the right, and a wide review sheet for the
 * editor and comparisons. Pure rules only; `task-layout.tsx` draws them.
 */

/** Window width bands (PLAN 1.4). */
export type LayoutBand = "wide" | "regular" | "narrow" | "compact";
export const BAND_WIDE_MIN = 1500;
export const BAND_REGULAR_MIN = 1100;
export const BAND_NARROW_MIN = 820;

export function layoutBand(width: number): LayoutBand {
  if (!Number.isFinite(width) || width >= BAND_WIDE_MIN) return "wide";
  if (width >= BAND_REGULAR_MIN) return "regular";
  if (width >= BAND_NARROW_MIN) return "narrow";
  return "compact";
}

/** Below 1100px the list folds to a rail and the results become a drawer. */
export function bandUsesDrawer(band: LayoutBand): boolean {
  return band === "narrow" || band === "compact";
}

/** Default column sizes for a band, in px. `right` 0: a drawer instead. */
export function bandSizes(band: LayoutBand): { left: number; right: number; centerMax: number } {
  switch (band) {
    case "wide": return { left: 264, right: 520, centerMax: 820 };
    case "regular": return { left: 240, right: 400, centerMax: 760 };
    default: return { left: 56, right: 0, centerMax: 760 };
  }
}

/** The right panel's tabs, in order. The preview tab comes first (C). */
export const RIGHT_TABS = ["preview", "changes", "files", "history"] as const;
export type RightTab = typeof RIGHT_TABS[number];

/** What the wide review sheet shows. */
export type SheetMode = "changes" | "editor";

/**
 * Where an editor-state panel request leads now that the dock is gone
 * (`editor-state.ts` keeps its "files" | "editor" | "changes" requests;
 * the controller itself is unchanged).
 */
export type LayoutTarget =
  | { kind: "sheet"; mode: SheetMode }
  | { kind: "tab"; tab: RightTab }
  | { kind: "story" }
  | { kind: "list" };
export function panelRequestTarget(panel: "files" | "editor" | "changes" | "conversation" | "chats", diffVisible: boolean): LayoutTarget {
  switch (panel) {
    case "editor": return { kind: "sheet", mode: diffVisible ? "changes" : "editor" };
    case "changes": return { kind: "tab", tab: "changes" };
    case "files": return { kind: "tab", tab: "files" };
    case "conversation": return { kind: "story" };
    case "chats": return { kind: "list" };
  }
}

/** The right panel's width: one value per viewer, kept in localStorage. */
export const RIGHT_WIDTH_KEY = "elsecrew.m5.layout.rightWidth.v1";
export const RIGHT_WIDTH_MIN = 320;
/** 320px up to half the window (never below the minimum). */
export function clampRightWidth(width: number, viewport: number): number {
  const max = Math.max(RIGHT_WIDTH_MIN, Math.floor(viewport * 0.5));
  if (!Number.isFinite(width)) return RIGHT_WIDTH_MIN;
  return Math.min(max, Math.max(RIGHT_WIDTH_MIN, Math.round(width)));
}
export function parseRightWidth(raw: string | null): number | null {
  if (raw === null || !/^\d{3,4}$/.test(raw)) return null;
  const value = Number(raw);
  return value >= RIGHT_WIDTH_MIN && value <= 4000 ? value : null;
}
/** Arrow keys move the separator by 16px (Shift: 64px); Home/End jump to the limits. */
export function keyedRightWidth(current: number, key: string, shift: boolean, viewport: number): number | null {
  const step = shift ? 64 : 16;
  // The separator sits on the panel's left edge: left widens it.
  if (key === "ArrowLeft") return clampRightWidth(current + step, viewport);
  if (key === "ArrowRight") return clampRightWidth(current - step, viewport);
  if (key === "Home") return clampRightWidth(viewport, viewport);
  if (key === "End") return RIGHT_WIDTH_MIN;
  return null;
}

/**
 * The tab a task opens on (PLAN 1.5). The preview leads when the task has a
 * page to show; otherwise what the task's state is about.
 */
export function defaultRightTab(phase: TaskPhase | null, hasPreview: boolean): RightTab {
  if (phase === "stopped" || phase === "discarded") return "history";
  if (hasPreview) return "preview";
  if (phase === null || phase === "draft") return "files";
  if (phase === "running" || phase === "attention") return "changes";
  return "changes";
}

/** Roving tab focus (←/→/Home/End) over the tabs in order. */
export function rovingIndex(count: number, current: number, key: string): number | null {
  if (count <= 0) return null;
  if (key === "Home") return 0;
  if (key === "End") return count - 1;
  if (key === "ArrowRight") return (current + 1) % count;
  if (key === "ArrowLeft") return (current + count - 1) % count;
  return null;
}

/**
 * Whether anything the app draws covers the preview area. The preview
 * (a separate view drawn above the page) must hide while it is covered
 * (PLAN 2.3 z-order). One list of causes, kept here.
 */
export interface CoverCauses {
  sheetOpen: boolean;
  /** Below 1100px the results live in a drawer; closed, the preview is not on screen. */
  drawerUsed: boolean;
  drawerOpen: boolean;
  dialogOpen: boolean;
  popoverOpen: boolean;
  rightTab: RightTab;
}
export function previewObscured(causes: CoverCauses): boolean {
  if (causes.rightTab !== "preview") return true;
  if (causes.drawerUsed && !causes.drawerOpen) return true;
  return causes.sheetOpen || causes.dialogOpen || causes.popoverOpen;
}
