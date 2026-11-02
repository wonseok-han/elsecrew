import { randomUUID } from "node:crypto";
import { lstat, mkdir, rename, rm, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { classifySessionDiagnostics } from "../src/m5/contracts/session-diagnostics-v1.ts";

/** Best-effort diagnostic evidence, never execution/recovery authority.
 * One in-flight write and one latest pending snapshot; no transcript queue.
 */
export function createSessionDiagnosticLog(directory: string, kind: "provider" | "transport" = "provider") {
  const fileName = kind === "transport" ? "last-transport-progress.json" : "last-provider-checkpoints.json";
  const sessionId = randomUUID();
  let previous = "";
  let checkpoints: { claudeCheckpoints: readonly string[]; codexCheckpoints: readonly string[] } = { claudeCheckpoints: [], codexCheckpoints: [] };
  const transport: { method: string; requestId: string | null; phase: string; at: string }[] = [];
  let pending: { body: string; signature: string } | null = null;
  let draining: Promise<void> | null = null;
  async function drain(): Promise<void> {
    while (pending !== null) {
      const { body, signature } = pending;
      pending = null;
      const temporary = join(directory, `.provider-checkpoints-${randomUUID()}.tmp`);
      let attemptedWrite = false;
      try {
        await mkdir(directory, { recursive: true, mode: 0o700 });
        const info = await lstat(directory);
        if (info.isSymbolicLink() || !info.isDirectory()) continue;
        // Reject pre-existing directory links. Hostile same-UID directory swaps
        // between these path operations are outside this diagnostic guarantee.
        attemptedWrite = true;
        await writeFile(temporary, body, { flag: "wx", mode: 0o600 });
        await rename(temporary, join(directory, fileName));
        previous = signature;
      } catch { /* Disk/log failures must not block the app or grant authority. */ }
      finally { if (attemptedWrite) await rm(temporary, { force: true }).catch(() => {}); }
    }
  }
  function startDrain(): void {
    if (!draining) draining = drain().finally(() => {
      draining = null;
      if (pending !== null) startDrain();
    });
  }
  function capture(tail: string): void {
    const complete = tail.slice(0, tail.lastIndexOf("\n") + 1);
    const { claudeCheckpoints, codexCheckpoints = [] } = classifySessionDiagnostics(complete);
    if (!claudeCheckpoints.length && !codexCheckpoints.length) return;
    checkpoints = { claudeCheckpoints, codexCheckpoints };
    schedule();
  }
  function schedule(): void {
    const signature = JSON.stringify({ ...checkpoints, ...(transport.length ? { transport } : {}) });
    if (signature === previous && !draining) return;
    pending = { signature, body: JSON.stringify({ schemaVersion: 1, sessionId, capturedAt: new Date().toISOString(), ...checkpoints, ...(transport.length ? { transport } : {}) }) + "\n" };
    startDrain();
  }
  function captureTransport(method: string, requestId: string | null, phase: string): void {
    // Only generated IPC correlation IDs and fixed lifecycle labels. No payload,
    // provider run IDs, file paths, errors, model output, or credentials.
    const event = method === "agent.runStateChanged";
    if (event ? requestId !== null || !["received", "forwarded"].includes(phase)
      : !["agent.getCapabilities", "run.startAgent", "chat.get"].includes(method)
        || typeof requestId !== "string" || !/^m5_[0-9a-f]{8}(?:-[0-9a-f]{4}){3}-[0-9a-f]{12}$/.test(requestId)
        || !["queued", "received", "resolved", "rejected"].includes(phase)) return;
    transport.push({ method, requestId, phase, at: new Date().toISOString() });
    if (transport.length > 64) transport.shift();
    schedule();
  }
  return { capture, captureTransport, async flush(): Promise<void> { while (draining) await draining; } };
}
