// First run checklist (ADE U2, DESIGN D11): three steps, each with its own
// button and the agents' connection shown where the person acts on it. Only
// the step to do now carries the action color.
import { useEffect, useState } from "react";
import { useI18n } from "../i18n.tsx";
import { firstRunStep } from "./first-run-step.ts";
import { CONNECTIONS_CHANGED, PROVIDER_AUTH_CHANGED, readConnections, type Connections } from "./provider-connections.ts";

export type FirstRunProvider = "codex" | "claude";
const AGENTS: readonly { login: FirstRunProvider; key: keyof Connections; name: string }[] = [
  { login: "codex", key: "codex", name: "Codex" },
  { login: "claude", key: "claude-code", name: "Claude Code" },
];

/** The agents' connections, followed as they change; null until first read. */
function useConnections(enabled: boolean): Connections | null {
  const [connections, setConnections] = useState<Connections | null>(null);
  useEffect(() => {
    if (!enabled) return;
    let active = true;
    const read = () => { void readConnections().then(value => { if (active) setConnections(value); }, () => undefined); };
    read();
    window.addEventListener(CONNECTIONS_CHANGED, read);
    window.addEventListener(PROVIDER_AUTH_CHANGED, read);
    return () => { active = false; window.removeEventListener(CONNECTIONS_CHANGED, read); window.removeEventListener(PROVIDER_AUTH_CHANGED, read); };
  }, [enabled]);
  return connections;
}

export function FirstRunChecklist({ disabled, ready, onChoose, onConnect }: { disabled: boolean; ready: boolean; onChoose: () => void; onConnect: (provider: FirstRunProvider) => void }) {
  const { t } = useI18n();
  const connections = useConnections(ready);
  const step = firstRunStep(connections);
  const firstMissing = AGENTS.find(agent => connections && !connections[agent.key])?.login;
  return <section className="m5-guided-start m5-first-run" aria-label={t("m5.start.title")} data-testid="first-run">
    <div className="m5-start-heading">
      <p className="m5-eyebrow">{t("m5.start.progress", { step })}</p>
      <h1>{t("m5.start.title")}</h1>
      <p>{t("m5.start.help")}</p>
    </div>
    <ol className="m5-steps m5-start-steps m5-card">
      <li className={step === 1 ? "m5-step-current" : "m5-step-done"}>
        <span className="m5-step-mark" aria-hidden="true">{step === 1 ? "1" : "✓"}</span>
        <div className="m5-step-body">
          <h3>{t("m5.start.connect")}</h3>
          <p>{t("m5.start.connectHelp")}</p>
          <ul className="m5-start-agents" aria-label={t("m5.start.agents")}>
            {AGENTS.map(agent => {
              const connected = connections?.[agent.key] === true;
              return <li key={agent.login} data-provider={agent.login}>
                <strong>{agent.name}</strong>
                <span className={`m5-badge ${connected ? "m5-badge-done" : ""}`.trim()}>{connections === null ? t("m5.start.checking") : connected ? t("m5.login.connected") : t("m5.connection.off")}</span>
                {!connected && <button type="button" className={step === 1 && firstMissing === agent.login ? "m5-btn-primary" : "m5-secondary"} disabled={!ready} aria-label={t("m5.start.connectNamed", { name: agent.name })} onClick={() => onConnect(agent.login)}>{t("m5.start.connectAction")}</button>}
              </li>;
            })}
          </ul>
        </div>
      </li>
      <li className={step === 2 ? "m5-step-current" : "m5-step-later"}>
        <span className="m5-step-mark" aria-hidden="true">2</span>
        <div className="m5-step-body"><h3>{t("m5.start.project")}</h3><p>{t("m5.start.projectHelp")}</p></div>
        <div className="m5-start-action"><button type="button" className={step === 2 ? "m5-btn-primary" : "m5-secondary"} onClick={onChoose} disabled={disabled}>{t("m5.shell.choose")}</button></div>
      </li>
      <li className="m5-step-later m5-step-locked">
        <span className="m5-step-mark" aria-hidden="true">3</span>
        <div className="m5-step-body"><h3>{t("m5.start.goal")}</h3><p>{t("m5.start.goalHelp")}</p></div>
        <span className="m5-step-lock">{t("m5.start.afterProject")}</span>
      </li>
    </ol>
    <p className="m5-settings-intro">{t("m5.start.projectLimit")}</p>
  </section>;
}
