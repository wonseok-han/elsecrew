import { randomUUID } from "node:crypto";
import { lstat, mkdir, rename, rm, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { classifySessionDiagnostics } from "../src/m5/contracts/session-diagnostics-v1.ts";

/** Best-effort diagnostic evidence, never execution/recovery authority.
 * One in-flight write and one latest pending snapshot; no transcript queue.
 */
export function createSessionDiagnosticLog(directory: string) {
  const sessionId = randomUUID();
  let previous = "";
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
        await rename(temporary, join(directory, "last-provider-checkpoints.json"));
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
    const signature = JSON.stringify({ claudeCheckpoints, codexCheckpoints });
    if (signature === previous && !draining) return;
    pending = { signature, body: JSON.stringify({ schemaVersion: 1, sessionId, capturedAt: new Date().toISOString(), claudeCheckpoints, codexCheckpoints }) + "\n" };
    startDrain();
  }
  return { capture, async flush(): Promise<void> { while (draining) await draining; } };
}
