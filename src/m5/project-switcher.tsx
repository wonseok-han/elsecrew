import type { ReactNode } from "react";
import { useI18n } from "../i18n.tsx";

/**
 * The open project at the top of the task list. The project list (the old
 * rail) folds into its popover; switching works as before: the open
 * project is closed first (1차-나; switching without closing is 2차 F2).
 */
export function ProjectSwitcher({ name, root, children }: { name: string; root: string; children: ReactNode }) {
  const { t } = useI18n();
  const initial = Array.from(name.trim() || "?")[0]!.toUpperCase();
  return <details className="m5-project-switcher" data-testid="project-switcher">
    <summary title={root} aria-label={t("m5.layout.switchProject", { name })}>
      <span className="m5-switcher-avatar" aria-hidden="true">{initial}</span>
      <span className="m5-switcher-name">{name}</span>
      <span className="m5-switcher-caret" aria-hidden="true">▾</span>
    </summary>
    <div className="m5-project-switcher-panel">{children}</div>
  </details>;
}
