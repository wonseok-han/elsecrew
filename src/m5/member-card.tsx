import { memo, useEffect, useState } from "react";
import { useI18n } from "../i18n.tsx";
import type { AgentRunRecordV1 } from "./contracts/agent-adapter-v1.ts";
import type { RoleDefinitionV3 } from "./contracts/run-recovery-v1.ts";
import { AgentOutputPanel } from "./agent-output-panel.tsx";
import { OrchestrationMessageBody } from "./orchestration-message-body.tsx";
import { roleLabel } from "./role-names.ts";
import { isLeadRole, roleColorIndex, roleInitial } from "./role-colors.ts";
import { runStageStatus } from "./task-story-model.ts";
import type { TaskPhase } from "./task-status.ts";
import { runDurationSeconds, runOutcomeNotice } from "./work-status.ts";

interface CardMessage { messageId: string; kind: string; roleId: string | null; runId: string | null; text: string; createdAt: string }

/** The role's icon: its first letter on its colour (same everywhere, R5). */
export function RoleIcon({ roleId, name }: { roleId: string; name: string }) {
  const { t } = useI18n();
  return <>
    <span className="m5-role-icon" data-role-color={roleColorIndex(roleId)} aria-hidden="true">{roleInitial(name)}</span>
    {/* The lead mark comes from the role ID, so a member named like the coordinator still reads apart (review m1). */}
    {isLeadRole(roleId) && <span className="m5-role-lead" title={t("m5.card.lead")} aria-label={t("m5.card.lead")}>★</span>}
  </>;
}

/**
 * A team member's card (ADE 1차-나 L4b): one run and the report it posted,
 * as one card. Header: role, state, time. Body: the report folded to its
 * summary (1차-가 renderer), or the run's closing line. The member working
 * now shows its live activity. Provider, model, run ID and exit details are
 * in the History tab, not here.
 */
export const MemberCard = memo(function MemberCard({ run, reports, definitions, teamRoleIds, phase, clock }: {
  run: AgentRunRecordV1;
  reports: readonly CardMessage[];
  definitions?: readonly RoleDefinitionV3[];
  /** Roles in the project's team now; a role no longer there says so. */
  teamRoleIds: ReadonlySet<string> | null;
  phase: TaskPhase | null;
  clock: Intl.DateTimeFormat;
}) {
  const { t } = useI18n();
  const status = runStageStatus(run);
  const live = status === "now";
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    if (!live) return;
    const timer = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(timer);
  }, [live]);
  const roleId = run.selectedProfile.roleId;
  const name = roleLabel(roleId, definitions, t);
  const seconds = runDurationSeconds(run, now);
  const duration = seconds === null ? "" : seconds >= 60 ? t("m5.run.duration.minutes", { m: Math.floor(seconds / 60), s: seconds % 60 }) : t("m5.run.duration.seconds", { s: seconds });
  const started = Date.parse(run.createdAt);
  const notice = reports.length === 0 ? runOutcomeNotice(run, phase) : null;
  const gone = teamRoleIds !== null && !teamRoleIds.has(roleId);
  return <article className={`m5-member-card m5-member-${status}`} data-run-id={run.runId} data-role-id={roleId} data-testid="member-card" aria-label={name}>
    <header className="m5-member-head">
      <RoleIcon roleId={roleId} name={name} />
      <strong className="m5-member-name" title={name}><bdi>{name}</bdi></strong>
      {live ? <span className="m5-pill m5-pill-running">{t("m5.card.now")}</span>
        : <span className={`m5-member-state m5-member-state-${status}`}>{t(`m5.stage.status.${status}`)}{duration ? ` · ${duration}` : ""}</span>}
      {live && duration && <span className="m5-member-state">{duration}</span>}
      <time className="m5-member-time" dateTime={run.createdAt}>{Number.isFinite(started) ? clock.format(new Date(started)) : ""}</time>
    </header>
    {gone && <p className="m5-member-gone">{t("m5.card.roleGone")}</p>}
    {reports.map(report => <div key={report.messageId} className="m5-member-report">
      <OrchestrationMessageBody text={report.text} role={report.roleId} definitions={definitions} fold={report.kind === "role"} />
    </div>)}
    {notice && <p className="m5-member-note">{t(notice)}</p>}
    {run.failure && <p className="m5-member-failure" role="alert">{t(`m5.run.failure.${run.failure.stage}`)}</p>}
    {live && <div className="m5-member-live"><AgentOutputPanel runs={[run]} definitions={definitions} posted={reports} /></div>}
  </article>;
});

/** A message that belongs to no run in this task: a status line or an older report. */
export const StoryMessageCard = memo(function StoryMessageCard({ message, definitions, clock }: { message: CardMessage; definitions?: readonly RoleDefinitionV3[]; clock: Intl.DateTimeFormat }) {
  const { t } = useI18n();
  const name = message.roleId ? roleLabel(message.roleId, definitions, t) : t("m5.chat.status");
  return <article className={`m5-member-card m5-story-message m5-message-${message.kind}`} data-run-id={message.runId ?? undefined}>
    <header className="m5-member-head">
      {message.roleId ? <RoleIcon roleId={message.roleId} name={name} /> : null}
      <strong className="m5-member-name"><bdi>{name}</bdi></strong>
      <time className="m5-member-time" dateTime={message.createdAt}>{clock.format(new Date(message.createdAt))}</time>
    </header>
    <OrchestrationMessageBody text={message.text} role={message.roleId} definitions={definitions} fold={message.kind === "role"} />
  </article>;
});

/** The person's own follow-up, as a bubble on the right. */
export const UserBubble = memo(function UserBubble({ message, clock }: { message: CardMessage; clock: Intl.DateTimeFormat }) {
  const { t } = useI18n();
  return <article className="m5-user-bubble" data-testid="user-bubble">
    <span className="m5-user-who">{t("m5.chat.me")} · <time dateTime={message.createdAt}>{clock.format(new Date(message.createdAt))}</time></span>
    <OrchestrationMessageBody text={message.text} role={null} />
  </article>;
});
