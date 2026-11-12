import { useI18n } from "../i18n.tsx";
import { MessageBody } from "./message-body.tsx";
import { orchestratorDelegation, orchestratorMessage } from "./orchestrator-message.ts";

export function OrchestrationMessageBody({ text, role }: { text: string; role: string | null }) {
  const { t } = useI18n();
  const delegated = orchestratorDelegation(text, role);
  return <>
    <MessageBody text={orchestratorMessage(text, role)} />
    {delegated && <details className="m5-delegation-details"><summary>{t("m5.chat.delegatedTask", { role: t(`m5.roles.${delegated.role}`) })}</summary><MessageBody text={delegated.instruction} /></details>}
  </>;
}
