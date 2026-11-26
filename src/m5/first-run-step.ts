import type { Connections } from "./provider-connections.ts";

/** Which first-run step is the one to do now: connect an agent, then pick a project (ADE U2, D11). */
export function firstRunStep(connections: Connections | null): 1 | 2 {
  return connections && (connections.codex || connections["claude-code"]) ? 2 : 1;
}
