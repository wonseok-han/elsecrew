/**
 * Build-then-preview host (결과 미리보기 확장 1단계): what the build runner
 * needs from this computer — finding `node` and the package manager (and
 * who they are), the workspace folder check, the macOS sandbox each job
 * runs in, starting the job as the leader of its own session and group,
 * listing and signalling only its own processes, the record of running jobs
 * for the next start, the output check and where the masked log is kept.
 * main.ts only hands in the sidecar, its own consent box and the app data
 * folders (main reads no environment itself).
 */
import { execFileSync, spawn } from "node:child_process";
import { createHash } from "node:crypto";
import { accessSync, constants, existsSync, lstatSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, realpathSync, rmSync, statSync, writeFileSync } from "node:fs";
import { homedir, tmpdir } from "node:os";
import { dirname, isAbsolute, join } from "node:path";
import { PreviewBuildManagerV1, type PreviewBuildSidecarV1, type PreviewProcessV1, type PreviewRunningRecordV1 } from "./preview-build.ts";
import { findTool, newestNodeVersions, previewBuildCaches, previewBuildConsentBox, previewBuildHiddenPaths, previewBuildSandboxProfile, previewToolDirs, type PreviewBuildBoxV1 } from "./preview-build-policy.ts";

export interface PreviewBuildHostOptionsV1 {
  enabled: boolean;
  sidecar(): PreviewBuildSidecarV1 | undefined;
  /** main's native box; the index of the button the person chose. */
  ask(box: PreviewBuildBoxV1): Promise<number>;
  korean(): boolean;
  dataDirectory(): string;
  /** Folders a job may never read or write (the app's own data). */
  hiddenFolders(): string[];
  log(label: string): void;
}

const SANDBOX_EXEC = "/usr/bin/sandbox-exec";
const PS = "/bin/ps";

/** A regular file the person may run (no folder, no broken link). */
function isExecutableFile(path: string): boolean {
  try { accessSync(path, constants.X_OK); return statSync(path).isFile(); } catch { return false; }
}

/** Who a program is: its real file and that file's identity. */
function fileIdentity(path: string): string {
  const real = realpathSync(path);
  const stat = statSync(real);
  return `${real}|${stat.dev}|${stat.ino}|${stat.size}|${stat.ctimeMs}`;
}

/** The person's own package manager settings (they apply to the job): which
 * exist, and their bytes' hash for the tool identity (re-review B1). */
const USER_CONFIGS = [".npmrc", ".yarnrc", ".yarnrc.yml", ".config/pnpm/rc", "Library/Preferences/pnpm/rc"];
function userConfigs(home: string): { names: string[]; identity: string } {
  const names: string[] = [];
  const hashes: string[] = [];
  for (const name of USER_CONFIGS) {
    const path = join(home, name);
    try {
      const bytes = readFileSync(path);
      names.push(`~/${name}`);
      hashes.push(`${name}=${createHash("sha256").update(bytes).digest("hex")}`);
    } catch { /* absent */ }
  }
  return { names, identity: hashes.join("|") };
}

export function findPreviewBuildTools(manager: "npm" | "pnpm" | "yarn"): { program: string; pathDirs: string[]; identity: string; userConfigs: string[] } | null {
  const home = homedir();
  let versions: string[] = [];
  try { versions = newestNodeVersions(readdirSync(join(home, ".nvm/versions/node"))); } catch { /* no nvm */ }
  const dirs = previewToolDirs(process.env.PATH, home, versions);
  const program = findTool(manager, dirs, isExecutableFile);
  // The `node` next to the package manager first (nvm, Homebrew), then any.
  const node = findTool("node", [dirname(program ?? "/"), ...dirs], isExecutableFile);
  if (!program || !node) return null;
  try {
    const configs = userConfigs(home);
    return { program, pathDirs: [dirname(program), dirname(node), "/usr/bin", "/bin", "/usr/sbin", "/sbin"],
      identity: `${fileIdentity(program)}#${fileIdentity(node)}#${configs.identity}`, userConfigs: configs.names };
  } catch { return null; }
}

/** This user's processes: id, parent, group and start time (`ps -x`). */
export function listOwnProcesses(): PreviewProcessV1[] {
  const text = execFileSync(PS, ["-x", "-o", "pid=,ppid=,pgid=,lstart="], { encoding: "utf8", maxBuffer: 16 * 1024 * 1024 });
  const rows: PreviewProcessV1[] = [];
  for (const line of text.split("\n")) {
    const match = /^\s*(\d+)\s+(\d+)\s+(\d+)\s+(.+?)\s*$/.exec(line);
    if (match) rows.push({ pid: Number(match[1]), ppid: Number(match[2]), pgid: Number(match[3]), started: match[4]! });
  }
  return rows;
}

function startedAt(pid: number): string | null {
  try { return execFileSync(PS, ["-o", "lstart=", "-p", String(pid)], { encoding: "utf8" }).trim() || null; } catch { return null; }
}

export function isSameProcessAlive(pid: number, started: string): boolean {
  return Number.isSafeInteger(pid) && pid > 1 && startedAt(pid) === started;
}

/** `<workspace>/<output>/index.html`: every step a real folder (no links),
 * the page a regular file changed at or after `since`. */
export function outputWritten(workspace: string, output: string, since: number): boolean {
  try {
    let path = workspace;
    for (const part of output.split("/")) {
      path = join(path, part);
      const stat = lstatSync(path);
      if (stat.isSymbolicLink() || !stat.isDirectory()) return false;
    }
    const page = lstatSync(join(path, "index.html"));
    return page.isFile() && !page.isSymbolicLink() && page.mtimeMs >= since;
  } catch { return false; }
}

/** Each hidden place as named and, when it leads elsewhere (a link to a
 * key folder), its real target too: the sandbox checks real paths
 * (re-review B2). */
export function hiddenWithTargets(paths: readonly string[]): string[] {
  const out: string[] = [];
  for (const path of paths) {
    out.push(path);
    try { const real = realpathSync(path); if (real !== path) out.push(real); } catch { /* absent */ }
  }
  return [...new Set(out)];
}

export function createPreviewBuildManager(options: PreviewBuildHostOptionsV1): PreviewBuildManagerV1 {
  const runningFile = () => join(options.dataDirectory(), "preview-builds", "running.json");
  const manager = new PreviewBuildManagerV1({
    enabled: options.enabled,
    sidecar: options.sidecar,
    confirm: async (consent) => (await options.ask(previewBuildConsentBox(consent, options.korean()))) === 1,
    findTools: findPreviewBuildTools,
    // The sidecar's recorded folder, as it is: a real directory, no link on the way.
    isWorkspaceFolder: (path) => {
      try { return isAbsolute(path) && !lstatSync(path).isSymbolicLink() && statSync(path).isDirectory() && realpathSync(path) === path; } catch { return false; }
    },
    places: (_step, manager, yarnBerry) => {
      const home = realpathSync(homedir());
      return { caches: previewBuildCaches(manager, yarnBerry, home), hidden: hiddenWithTargets(previewBuildHiddenPaths(home, options.hiddenFolders())) };
    },
    // This job's own TMPDIR, inside the user's temporary folder (re-review B2).
    makeTemp: () => mkdtempSync(join(realpathSync(tmpdir()), "elsecrew-build-")),
    removeTemp: (path) => { if (/\/elsecrew-build-[^/]+$/.test(path)) rmSync(path, { recursive: true, force: true }); },
    confine: (step, workspace, temp, places, program, args) => {
      if (process.platform !== "darwin" || !isExecutableFile(SANDBOX_EXEC)) return null;
      try {
        const profile = previewBuildSandboxProfile(step, { workspace, temp, caches: places.caches, hidden: places.hidden });
        return { program: SANDBOX_EXEC, args: ["-p", profile, program, ...args] };
      } catch { return null; }
    },
    spawn: (program, args, spawnOptions) => spawn(program, args, spawnOptions),
    // Only the group the job itself leads (it is started detached).
    killGroup: (pid, signal) => { process.kill(-pid, signal); },
    killProcess: (pid, started, signal) => { if (isSameProcessAlive(pid, started)) process.kill(pid, signal); },
    processes: listOwnProcesses,
    isAlive: isSameProcessAlive,
    saveRunning: (records) => {
      mkdirSync(dirname(runningFile()), { recursive: true, mode: 0o700 });
      writeFileSync(runningFile(), JSON.stringify(records), { mode: 0o600 });
    },
    loadRunning: () => {
      if (!existsSync(runningFile())) return null;
      const parsed: unknown = JSON.parse(readFileSync(runningFile(), "utf8"));
      if (!Array.isArray(parsed)) return [];
      return parsed.filter((record): record is PreviewRunningRecordV1 => typeof record === "object" && record !== null && Array.isArray((record as PreviewRunningRecordV1).processes))
        .map(record => ({ leader: Number(record.leader), ...(typeof record.leaderStarted === "string" ? { leaderStarted: record.leaderStarted } : {}),
          processes: record.processes.filter(entry => Number.isSafeInteger(entry?.pid) && typeof entry?.started === "string") }));
    },
    outputWritten,
    environment: process.env,
    writeLog: (name, text) => {
      const folder = join(options.dataDirectory(), "preview-builds");
      mkdirSync(folder, { recursive: true, mode: 0o700 });
      writeFileSync(join(folder, name), text, { mode: 0o600 });
    },
    log: options.log,
  });
  return manager;
}
