import { useI18n } from "../i18n.tsx";
import { EmptyState } from "./empty-state.tsx";
import type { ChatUiController } from "./chat-state.ts";
import { AgentRuns } from "./task-story.tsx";
import type { TaskPhase } from "./task-status.ts";

/**
 * The History tab (ADE 1차-나 L5): this task's runs in order with their
 * role, provider, model, start, duration, exit and run ID, the saved output
 * of any run, earlier runs folded, and the help for a failed run. The
 * development-only sample runs sit folded at the bottom.
 */
export function HistoryTab({ controller, chat, admissionReady, fixtureAvailable, phase }: {
  controller: ChatUiController;
  chat: ReturnType<ChatUiController["snapshot"]>;
  admissionReady: boolean;
  fixtureAvailable: boolean;
  phase: TaskPhase | null;
}) {
  const { t } = useI18n();
  if (!chat.activeChat) return <EmptyState icon="clock" title={t("m5.history.noneTitle")} help={t("m5.history.none")} />;
  const fixtureActive = Boolean(chat.activeRunId);
  const actualActive = chat.agentRuns.some(run => ["preparing", "running", "cancelling"].includes(run.state));
  const blocked = controller.isActionBlocked();
  return <section className="m5-history-tab" aria-label={t("m5.layout.tab.history")}>
    {chat.agentRuns.length === 0 ? <EmptyState icon="clock" title={t("m5.history.empty")} help={t("m5.history.emptyHelp")} />
      : <AgentRuns runs={chat.agentRuns} definitions={chat.activeChat.roleDefinitions} posted={chat.messages} phase={phase} />}
    {fixtureAvailable && <details className="m5-history-dev">
      <summary>{t("m5.history.dev")}</summary>
      <div className="m5-demo-controls">
        <span>{t("m5.chat.demoHelp")}</span>
        {(["planning", "coding", "review"] as const).map(role => <button key={`demo-${role}`} type="button" disabled={!admissionReady || fixtureActive || actualActive || chat.agentStartingRoleId !== null || blocked} onClick={() => void controller.startDemo(role)}>{t("m5.chat.demo", { role: t(`m5.roles.${role}`) })}</button>)}
      </div>
    </details>}
  </section>;
}
