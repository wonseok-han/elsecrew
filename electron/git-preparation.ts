import { execFile } from "node:child_process";
import { validateProjectCatalogRequest, validateProjectCatalogResult, type ProjectCatalogRequest, type ProjectCatalogResult } from "../src/m5/contracts/project-catalog-v1.ts";
import type { SidecarLaunchV1 } from "./sidecar-client.ts";

export const WINDOWS_GIT_PREPARATION_ENABLED = false; // Native integration review before product activation.
export function isGitPreparation(input: ProjectCatalogRequest): boolean {
  return input.action === "inspectGitTrust" || input.action === "confirmGitTrust" || input.action === "revokeGitTrust";
}

/** One owner serializes consent mutations; only the main-selected sidecar runs. */
export class GitPreparationClient {
  #queue: Promise<void> = Promise.resolve();
  #closed = false;
  #pending = 0;
  get pending(): boolean { return this.#pending > 0; }
  reopen(): void { this.#closed = false; }
  constructor(private readonly launch: SidecarLaunchV1) {}
  request(input: ProjectCatalogRequest, assertOwner: () => void): Promise<ProjectCatalogResult> {
    const request = validateProjectCatalogRequest(input);
    if (!isGitPreparation(request)) return Promise.reject(new Error("operation is not Git preparation"));
    const body = JSON.stringify(request);
    if (Buffer.byteLength(body) > 65536) return Promise.reject(new Error("Git preparation request exceeds limit"));
    this.#pending++;
    const operation = this.#queue.then(async () => {
      if (this.#closed) throw new Error("Git preparation is closing");
      assertOwner();
      const output = await new Promise<string>((resolve, reject) => {
        const child = execFile(this.launch.command, ["--git-preparation"], {
          cwd: this.launch.cwd, windowsHide: true, shell: false,
          timeout: 30_000, maxBuffer: 65_536, encoding: "utf8",
        }, (error, stdout) => {
          if (error) reject(new Error("Git preparation did not complete"));
          else resolve(stdout);
        });
        child.stdin?.on("error", () => {}); // execFile close callback owns completion.
        child.stdin?.end(body);
      });
      assertOwner();
      return validateProjectCatalogResult(JSON.parse(output), request);
    });
    this.#queue = operation.then(() => {}, () => {}).finally(() => { this.#pending--; });
    return operation;
  }
  async close(): Promise<void> {
    this.#closed = true;
    await this.#queue; // At most the active bounded request; queued requests reject.
  }
}
