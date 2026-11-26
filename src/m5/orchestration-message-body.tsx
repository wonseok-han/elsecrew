import { useI18n } from "../i18n.tsx";
import { MessageBody } from "./message-body.tsx";
import { orchestratorDelegation, orchestratorMessage, orchestratorMisdirected } from "./orchestrator-message.ts";
import { roleLabel } from "./role-names.ts";
import type { RoleDefinitionV3 } from "./contracts/run-recovery-v1.ts";

export function OrchestrationMessageBody({ text, role, definitions, fold = false }: { text: string; role: string | null; definitions?: readonly RoleDefinitionV3[]; fold?: boolean }) {
  const { t } = useI18n();
  const team = definitions?.map(definition => definition.roleId);
  const delegated = orchestratorDelegation(text, role, team);
  const misdirected = orchestratorMisdirected(text, role, team);
  if (misdirected !== null) return <p className="m5-misdirected">{t("m5.chat.misdirected", { role: `\u2068${misdirected}\u2069` })}</p>;
  return <>
    <MessageBody text={orchestratorMessage(text, role, team)} fold={fold} />
    {delegated && <details className="m5-delegation-details"><summary>{t("m5.chat.delegatedTask", { role: roleLabel(delegated.role, definitions, t) })}</summary><MessageBody text={delegated.instruction} /></details>}
  </>;
}
