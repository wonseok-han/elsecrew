/**
 * Build-then-preview (결과 미리보기 확장 1단계): what the preview tab's build
 * bar says and allows. Pure; tested in `tests/m5-preview-build.test.mts`.
 * Main decides and runs (electron/preview-build.ts); the tab only asks.
 */
import { PREVIEW_BUILD_LOG_MAX_LINES_V1, type PreviewBuildJobViewV1, type PreviewBuildPlanViewV1, type PreviewBuildStartOutcomeV1, type PreviewBuildStepV1 } from "./contracts/preview-v1.ts";

export type PreviewBuildNote = "notIgnored" | "toolMissing" | "unsupported";
export type PreviewBuildStatus =
  | "installed" | "installFailed" | "built" | "buildFailed" | "cancelled" | "timedOut"
  | "declined" | "busy" | "refused" | "changed" | "cancelledStart" | "leftovers";
/** A start main did not take (`started` is not a status). */
export type PreviewBuildStartNote = Exclude<PreviewBuildStartOutcomeV1, "started">;

export type PreviewBuildBarView =
  /** Not a package with a build (or no answer): the bar is not shown. */
  | { kind: "hidden" }
  | { kind: "note"; note: PreviewBuildNote }
  | { kind: "ready"; running: PreviewBuildStepV1 | null; needsInstall: boolean; canInstall: boolean; canBuild: boolean; teamWorking: boolean; buildCommand: string; installCommand: string };

export function previewBuildBar(plan: PreviewBuildPlanViewV1 | null, job: PreviewBuildJobViewV1 | null, teamWorking: boolean): PreviewBuildBarView {
  if (!plan) return { kind: "hidden" };
  switch (plan.refusal) {
    case null: break;
    case "dependenciesNotIgnored": case "outputNotIgnored": return { kind: "note", note: "notIgnored" };
    case "toolMissing": return { kind: "note", note: "toolMissing" };
    case "unsupportedManager": case "unsafeOutput": case "invalidPackage": case "linkedInput": case "unsupportedSettings": return { kind: "note", note: "unsupported" };
    // No package, no build script, an old copy, or no answer: nothing to offer.
    default: return { kind: "hidden" };
  }
  if (!plan.buildCommand || !plan.installCommand) return { kind: "hidden" };
  const running = job?.state === "running" ? job.step : null;
  const free = running === null && !teamWorking;
  return {
    kind: "ready", running, needsInstall: plan.needsInstall, teamWorking,
    canInstall: free, canBuild: free && !plan.needsInstall,
    buildCommand: plan.buildCommand, installCommand: plan.installCommand,
  };
}

/** The line under the bar after a job ended or a start was not taken. */
export function previewBuildStatus(job: PreviewBuildJobViewV1 | null, start: PreviewBuildStartNote | null): PreviewBuildStatus | null {
  if (start) return start === "cancelled" ? "cancelledStart" : start;
  // Processes that did not end outrank how the job ended (PR #22 review B3).
  if (job && job.leftovers > 0 && job.state !== "running") return "leftovers";
  if (!job || job.state === "idle" || job.state === "running") return null;
  if (job.state === "cancelled") return "cancelled";
  if (job.state === "timedOut") return "timedOut";
  if (job.step === "install") return job.state === "succeeded" ? "installed" : "installFailed";
  return job.state === "succeeded" ? "built" : "buildFailed";
}

/** The log as the tab keeps it: the newest lines, bounded. */
export function appendBuildLines(previous: readonly string[], lines: readonly string[]): string[] {
  const next = [...previous, ...lines];
  return next.length > PREVIEW_BUILD_LOG_MAX_LINES_V1 ? next.slice(next.length - PREVIEW_BUILD_LOG_MAX_LINES_V1) : next;
}

/** A job that just ended well: a build opens its output, an install asks for the plan again. */
export function previewBuildFinished(before: PreviewBuildJobViewV1 | null, after: PreviewBuildJobViewV1): "built" | "installed" | "ended" | null {
  if (before?.state !== "running" || after.state === "running") return null;
  if (after.state !== "succeeded") return "ended";
  if (after.step === "build" && after.outputReady) return "built";
  return after.step === "install" ? "installed" : "ended";
}
