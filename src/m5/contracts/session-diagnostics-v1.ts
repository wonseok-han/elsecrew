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
export type SessionDiagnostics = { scope: "currentAppSession"; claudeCheckpoints: string[] };

export function validateSessionDiagnostics(input: unknown): SessionDiagnostics {
  if (!input || typeof input !== "object" || Array.isArray(input)) throw new Error("invalid session diagnostics");
  const value = input as Record<string, unknown>;
  if (Object.keys(value).sort().join() !== "claudeCheckpoints,scope" || value.scope !== "currentAppSession"
    || !Array.isArray(value.claudeCheckpoints) || value.claudeCheckpoints.length > 32
    || !value.claudeCheckpoints.every(stage => typeof stage === "string" && (stages as readonly string[]).includes(stage))) {
    throw new Error("invalid session diagnostics");
  }
  return {scope:"currentAppSession", claudeCheckpoints:[...value.claudeCheckpoints]};
}

export function classifySessionDiagnostics(stderr: string): SessionDiagnostics {
  const checkpoints: string[] = [];
  // The sidecar tail is bounded already; defend this pure helper independently.
  const tail = stderr.slice(-32768);
  for (const line of tail.split(/\r?\n/)) {
    const prefix = "native Claude transport failed at ";
    if (!line.startsWith(prefix)) continue;
    const stage = line.slice(prefix.length);
    if ((stages as readonly string[]).includes(stage)) checkpoints.push(stage);
  }
  return {scope:"currentAppSession", claudeCheckpoints:checkpoints.slice(-32)};
}
