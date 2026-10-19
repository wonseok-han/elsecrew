export const SESSION_DIAGNOSTICS_CHANNEL = "m5:session-diagnostics:v1";
// Exact local checkpoint names only. Never accept free-form provider output.
const stages = ["start binding", "control initialization", "profile matching", "owner preparation",
  "prompt admission", "handoff cancellation", "session initialization", "session output",
  "session input", "session channel", "session receipt", "session process observation",
  "session persistence", "session handler", "channel state", "channel send", "channel receive",
  "session state", "session budget", "session tag", "session before init", "session event",
  "init expected", "init shape", "init identity", "init cwd", "init model", "init permission",
  "init tools", "init extensions", "init mcp", "init skills", "init plugins",
  "init plugins disabled builtins", "init plugins organization guard", "init plugins telemetry",
  "init plugins other builtins", "init plugins other registration", "init agents",
  "init plugin errors", "init capabilities"] as const;
const codexStages = [
  "turn start state", "turn start envelope", "turn start notification shape", "turn start notification correlation",
  "turn start notification state", "turn start reply shape", "turn start reply correlation", "turn start reply state",
  "turn start stream binding", "turn stream thread status", "turn stream token usage", "turn stream started",
  "turn stream completed", "turn stream item started", "turn stream item completed", "turn stream message delta",
  "turn stream reasoning", "turn stream request resolved", "turn stream provider error", "turn stream diff metadata",
  "turn stream plan metadata", "turn stream thread name", "turn stream thread settings", "turn stream hook",
  "turn stream model metadata", "turn stream warning", "turn stream account metadata", "turn stream mcp metadata",
  "turn stream moderation metadata", "turn stream other frame",
] as const;
export type SessionDiagnostics = { scope: "currentAppSession"; claudeCheckpoints: string[]; codexCheckpoints?: string[] };

export function validateSessionDiagnostics(input: unknown): SessionDiagnostics {
  if (!input || typeof input !== "object" || Array.isArray(input)) throw new Error("invalid session diagnostics");
  const value = input as Record<string, unknown>;
  const keys = Object.keys(value).sort().join();
  if (!["claudeCheckpoints,scope", "claudeCheckpoints,codexCheckpoints,scope"].includes(keys) || value.scope !== "currentAppSession"
    || !Array.isArray(value.claudeCheckpoints) || value.claudeCheckpoints.length > 32
    || !value.claudeCheckpoints.every(stage => typeof stage === "string" && (stages as readonly string[]).includes(stage))) {
    throw new Error("invalid session diagnostics");
  }
  if ("codexCheckpoints" in value && (!Array.isArray(value.codexCheckpoints) || value.codexCheckpoints.length > 32
    || !value.codexCheckpoints.every(stage => typeof stage === "string" && (codexStages as readonly string[]).includes(stage)))) {
    throw new Error("invalid session diagnostics");
  }
  return {scope:"currentAppSession", claudeCheckpoints:[...value.claudeCheckpoints],
    ...("codexCheckpoints" in value ? {codexCheckpoints:[...(value.codexCheckpoints as string[])]} : {})};
}

export function classifySessionDiagnostics(stderr: string): SessionDiagnostics {
  const checkpoints: string[] = [];
  const codexCheckpoints: string[] = [];
  // The sidecar tail is bounded already; defend this pure helper independently.
  const tail = stderr.slice(-32768);
  for (const line of tail.split(/\r?\n/)) {
    const codexPrefix = "native Codex transport failed at ";
    if (line.startsWith(codexPrefix)) {
      const stage = line.slice(codexPrefix.length);
      if ((codexStages as readonly string[]).includes(stage)) codexCheckpoints.push(stage);
    }
    const prefix = "native Claude transport failed at ";
    if (!line.startsWith(prefix)) continue;
    const stage = line.slice(prefix.length);
    if ((stages as readonly string[]).includes(stage)) checkpoints.push(stage);
  }
  return {scope:"currentAppSession", claudeCheckpoints:checkpoints.slice(-32), codexCheckpoints:codexCheckpoints.slice(-32)};
}
