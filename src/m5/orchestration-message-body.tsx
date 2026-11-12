import { useI18n } from "../i18n.tsx";
import { MessageBody } from "./message-body.tsx";
import { orchestratorDelegation, orchestratorMessage } from "./orchestrator-message.ts";
import { roleLabel } from "./role-names.ts";
import type { RoleDefinitionV3 } from "./contracts/run-recovery-v1.ts";

export function OrchestrationMessageBody({ text, role, definitions }: { text: string; role: string | null; definitions?: readonly RoleDefinitionV3[] }) {
  const { t } = useI18n();
  const team = definitions?.map(definition => definition.roleId);
  const delegated = orchestratorDelegation(text, role, team);
  return <>
    <MessageBody text={orchestratorMessage(text, role, team)} />
    {delegated && <details className="m5-delegation-details"><summary>{t("m5.chat.delegatedTask", { role: roleLabel(delegated.role, definitions, t) })}</summary><MessageBody text={delegated.instruction} /></details>}
  </>;
}
