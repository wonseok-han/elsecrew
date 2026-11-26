/**
 * Build-then-preview host (결과 미리보기 확장 1단계): what the build runner
 * needs from this computer — finding `node` and the package manager, the
 * workspace folder check, starting the process as the leader of its own
 * group, signalling only that group, the environment it starts from and
 * where its masked log is kept. main.ts only hands in the sidecar, its own
 * consent box and the app data folder (main reads no environment itself).
 */
import { spawn } from "node:child_process";
import { accessSync, constants, lstatSync, mkdirSync, readdirSync, realpathSync, statSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import { dirname, isAbsolute, join } from "node:path";
import { PreviewBuildManagerV1, type PreviewBuildSidecarV1 } from "./preview-build.ts";
import { findTool, newestNodeVersions, previewBuildConsentBox, previewToolDirs, type PreviewBuildBoxV1 } from "./preview-build-policy.ts";

export interface PreviewBuildHostOptionsV1 {
  enabled: boolean;
  sidecar(): PreviewBuildSidecarV1 | undefined;
  /** main's native box; the index of the button the person chose. */
  ask(box: PreviewBuildBoxV1): Promise<number>;
  korean(): boolean;
  dataDirectory(): string;
  log(label: string): void;
}

/** A regular file the person may run (no folder, no broken link). */
function isExecutableFile(path: string): boolean {
  try { accessSync(path, constants.X_OK); return statSync(path).isFile(); } catch { return false; }
}

export function findPreviewBuildTools(manager: "npm" | "pnpm" | "yarn"): { program: string; pathDirs: string[] } | null {
  const home = homedir();
  let versions: string[] = [];
  try { versions = newestNodeVersions(readdirSync(join(home, ".nvm/versions/node"))); } catch { /* no nvm */ }
  const dirs = previewToolDirs(process.env.PATH, home, versions);
  const program = findTool(manager, dirs, isExecutableFile);
  // The `node` next to the package manager first (nvm, Homebrew), then any.
  const node = findTool("node", [dirname(program ?? "/"), ...dirs], isExecutableFile);
  if (!program || !node) return null;
  return { program, pathDirs: [dirname(program), dirname(node), "/usr/bin", "/bin", "/usr/sbin", "/sbin"] };
}

export function createPreviewBuildManager(options: PreviewBuildHostOptionsV1): PreviewBuildManagerV1 {
  return new PreviewBuildManagerV1({
    enabled: options.enabled,
    sidecar: options.sidecar,
    confirm: async (consent) => (await options.ask(previewBuildConsentBox(consent, options.korean()))) === 1,
    findTools: findPreviewBuildTools,
    // The sidecar's recorded folder, as it is: a real directory, no link on the way.
    isWorkspaceFolder: (path) => {
      try { return isAbsolute(path) && !lstatSync(path).isSymbolicLink() && statSync(path).isDirectory() && realpathSync(path) === path; } catch { return false; }
    },
    spawn: (program, args, spawnOptions) => spawn(program, args, spawnOptions),
    // Only the group the build itself leads (it is started detached).
    killGroup: (pid, signal) => { process.kill(-pid, signal); },
    environment: process.env,
    writeLog: (name, text) => {
      const folder = join(options.dataDirectory(), "preview-builds");
      mkdirSync(folder, { recursive: true, mode: 0o700 });
      writeFileSync(join(folder, name), text, { mode: 0o600 });
    },
    log: options.log,
  });
}
