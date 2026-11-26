import { existsSync, readFileSync } from "node:fs";

/**
 * The workbench screen's source, across the files it was split into (ADE
 * 1차-나). Source checks that guard 1차-가 rules read this instead of
 * `workbench.tsx` alone, so moving a block to another file keeps them.
 */
const WORKBENCH_FILES = [
  "workbench.tsx",
  "task-story.tsx",
  "editor-surface.tsx",
  "task-layout.tsx",
  "task-list.tsx",
  "member-card.tsx",
  "stage-bar.tsx",
  "result-box.tsx",
  "review-sheet.tsx",
  "history-tab.tsx",
  "project-switcher.tsx",
];

export function workbenchSource(): string {
  return WORKBENCH_FILES
    .map(name => new URL(`../src/m5/${name}`, import.meta.url))
    .filter(url => existsSync(url))
    .map(url => readFileSync(url, "utf8"))
    .join("\n");
}
