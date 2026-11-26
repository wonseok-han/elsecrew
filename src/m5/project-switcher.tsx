import type { ReactNode } from "react";
import { useI18n } from "../i18n.tsx";
import type { ProjectActivity } from "./project-activity.ts";

/**
 * The open project at the top of the task list. The project list (the old
 * rail) folds into its popover. Switching leaves the open project's teams at
 * work (ADE multi-project P1); a mark here says when another project has a
 * team at work or a request waiting.
 */
export function ProjectSwitcher({ name, root, others, children }: { name: string; root: string; others?: ProjectActivity; children: ReactNode }) {
  const { t } = useI18n();
  const mark = others && others.attention > 0 ? "attention" : others?.working ? "running" : null;
  const initial = Array.from(name.trim() || "?")[0]!.toUpperCase();
  return <details className="m5-project-switcher" data-testid="project-switcher">
    <summary title={root} aria-label={t("m5.layout.switchProject", { name })}>
      <span className="m5-switcher-avatar" aria-hidden="true">{initial}</span>
      <span className="m5-switcher-name">{name}</span>
      {mark && <span className={`m5-switcher-others m5-switcher-others-${mark}`} role="img" aria-label={mark === "attention" ? t("m5.switch.othersAttention", { count: others!.attention }) : t("m5.switch.othersWorking")} title={mark === "attention" ? t("m5.switch.othersAttention", { count: others!.attention }) : t("m5.switch.othersWorking")} />}
      <span className="m5-switcher-caret" aria-hidden="true">▾</span>
    </summary>
    <div className="m5-project-switcher-panel">{children}</div>
  </details>;
}
