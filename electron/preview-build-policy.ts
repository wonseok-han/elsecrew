/**
 * Build-then-preview policy (결과 미리보기 확장 1단계, 2026-11-27): the pure
 * decisions `PreviewBuildManagerV1` applies. No Electron import, so each rule
 * is unit tested directly (`tests/m5-preview-build.test.mts`).
 *
 * A build is NOT inside the preview's isolation: it is an ordinary process
 * running the project's own code with the person's rights. What keeps it
 * bounded is here:
 * - fixed argument lists per package manager (never a string for a shell),
 * - an allow-listed environment (the app's own settings and anything that
 *   looks like a key stay out; the person's npm config still applies, as in
 *   their own terminal),
 * - its log masked line by line with the work-history rule before it is
 *   kept (`maskSecrets`, PR #11) and cut to a bounded size.
 */
import { createHash } from "node:crypto";
import type { PreviewPackageManagerV1, PreviewBuildStepV1 } from "../src/m5/contracts/preview-v1.ts";
import { PREVIEW_BUILD_LINE_MAX_CHARS_V1 } from "../src/m5/contracts/preview-v1.ts";
import { maskSecrets, secretName } from "../src/m5/secret-shape.ts";

export const PREVIEW_INSTALL_TIMEOUT_MS = 10 * 60_000;
export const PREVIEW_BUILD_TIMEOUT_MS = 5 * 60_000;
/** After asking a stopped build to end, the whole group is killed. */
export const PREVIEW_BUILD_KILL_GRACE_MS = 3_000;
/** Builds running at once over all tasks (one per task). */
export const PREVIEW_BUILD_MAX_RUNNING = 2;
/** A line still being written that grows past this is cut. */
export const PREVIEW_BUILD_PENDING_MAX_CHARS = 64 * 1024;

/** The fixed arguments of one step. A lockfile is never rewritten: with one
 * the installation is frozen to it; without one none is created (npm, pnpm). */
export function previewBuildArgs(manager: PreviewPackageManagerV1, step: PreviewBuildStepV1, lockfile: boolean): string[] {
  if (step === "build") return ["run", "build"];
  switch (manager) {
    case "npm": return lockfile ? ["ci", "--no-audit", "--no-fund"] : ["install", "--no-package-lock", "--no-audit", "--no-fund"];
    case "pnpm": return lockfile ? ["install", "--frozen-lockfile"] : ["install", "--lockfile=false"];
    case "yarn": return lockfile ? ["install", "--frozen-lockfile"] : ["install"];
  }
}

/** How the command reads in the consent box and the tab. */
export function previewBuildCommandLine(manager: PreviewPackageManagerV1, step: PreviewBuildStepV1, lockfile: boolean): string {
  return [manager, ...previewBuildArgs(manager, step, lockfile)].join(" ");
}

/** Settings a build may see. Everything else of the app's environment
 * (ELSECREW_*, provider keys, tokens a terminal exported before starting the
 * app) is left out. */
const PASSED = [
  "HOME", "USER", "LOGNAME", "SHELL", "LANG", "LC_ALL", "LC_CTYPE", "LC_MESSAGES", "TMPDIR", "TZ",
  // Windows (the build is not enabled there yet; kept so the rule is whole).
  "SystemRoot", "SYSTEMROOT", "WINDIR", "COMSPEC", "PATHEXT", "APPDATA", "LOCALAPPDATA", "USERPROFILE", "TEMP", "TMP", "HOMEDRIVE", "HOMEPATH",
] as const;

export function previewBuildEnvironment(source: Readonly<Record<string, string | undefined>>, pathDirs: readonly string[], delimiter = ":"): Record<string, string> {
  const env: Record<string, string> = {};
  for (const name of PASSED) {
    const value = source[name];
    if (typeof value === "string" && value.length > 0 && !secretName(name)) env[name] = value;
  }
  env.PATH = [...new Set(pathDirs)].join(delimiter);
  // No colours, no update or funding notices, no browser opened by a tool.
  env.NO_COLOR = "1";
  env.FORCE_COLOR = "0";
  env.BROWSER = "none";
  env.NO_UPDATE_NOTIFIER = "1";
  env.npm_config_update_notifier = "false";
  env.npm_config_fund = "false";
  return env;
}

/** Folders to look in for `node` and the package manager when the app was
 * opened from the Finder (whose PATH is short): the inherited PATH first,
 * then the usual installers' places. nvm-style version folders are given
 * newest first by the caller (`versions`). */
export function previewToolDirs(inheritedPath: string | undefined, home: string, versions: readonly string[] = []): string[] {
  const dirs = (inheritedPath ?? "").split(":").filter(dir => dir.startsWith("/"));
  dirs.push(
    "/opt/homebrew/bin", "/usr/local/bin",
    `${home}/.volta/bin`, `${home}/.local/bin`, `${home}/Library/pnpm`, `${home}/.npm-global/bin`,
    ...versions.map(version => `${home}/.nvm/versions/node/${version}/bin`),
    "/usr/bin", "/bin",
  );
  return [...new Set(dirs)];
}

/** `v24.2.0` sorts after `v9.11.2`. */
export function newestNodeVersions(names: readonly string[]): string[] {
  const parse = (name: string) => /^v(\d+)\.(\d+)\.(\d+)$/.exec(name)?.slice(1).map(Number) ?? null;
  return names.filter(name => parse(name) !== null).sort((a, b) => {
    const [x, y] = [parse(a)!, parse(b)!];
    return y[0]! - x[0]! || y[1]! - x[1]! || y[2]! - x[2]!;
  });
}

/** The first folder holding an executable `name` (`isExecutable` checks a
 * regular file the person may run). */
export function findTool(name: string, dirs: readonly string[], isExecutable: (path: string) => boolean): string | null {
  for (const dir of dirs) {
    const path = `${dir}/${name}`;
    if (isExecutable(path)) return path;
  }
  return null;
}

// eslint-disable-next-line no-control-regex
const ANSI = /\u001b\[[0-9;?]*[ -/]*[@-~]|\u001b\][^\u0007\u001b]*(?:\u0007|\u001b\\)|\u001b[@-Z\\-_]/g;
// eslint-disable-next-line no-control-regex
const CONTROL = /[\u0000-\u0008\u000b-\u001f\u007f]/g;

/** One output line as it is kept: colours and controls off, the last
 * carriage-return frame of a progress line, secrets masked, cut. */
export function previewBuildLine(raw: string): string {
  const frames = raw.replace(ANSI, "").split("\r");
  let line = frames.reverse().find(frame => frame.trim().length > 0) ?? "";
  line = line.replace(CONTROL, "").replace(/\t/g, "  ");
  // Masked before it is cut, so a cut never leaves half a key unmasked.
  line = maskSecrets(line);
  return line.length > PREVIEW_BUILD_LINE_MAX_CHARS_V1 ? `${line.slice(0, PREVIEW_BUILD_LINE_MAX_CHARS_V1 - 1)}…` : line;
}

/** Splits a stream into whole lines; the rest waits for more. */
export class PreviewLineSplitter {
  #pending = "";
  push(chunk: string): string[] {
    this.#pending += chunk;
    const parts = this.#pending.split("\n");
    this.#pending = parts.pop() ?? "";
    if (this.#pending.length > PREVIEW_BUILD_PENDING_MAX_CHARS) {
      parts.push(this.#pending.slice(0, PREVIEW_BUILD_PENDING_MAX_CHARS));
      this.#pending = "";
    }
    return parts;
  }
  flush(): string[] {
    const rest = this.#pending;
    this.#pending = "";
    return rest.length > 0 ? [rest] : [];
  }
}

/** The kept log's file name: no project or chat name in it. */
export function previewBuildLogName(projectId: string, chatId: string): string {
  return `${createHash("sha256").update(`${projectId}\u0000${chatId}`).digest("hex").slice(0, 32)}.log`;
}

/** The words of main's consent box (PR #12·#14: main asks, never the page). */
export interface PreviewBuildBoxV1 { message: string; detail: string; buttons: string[] }
export function previewBuildConsentBox(consent: { step: PreviewBuildStepV1; command: string; program: string; folder: string; buildScript: string | null }, korean: boolean): PreviewBuildBoxV1 {
  const install = consent.step === "install";
  if (korean) {
    return {
      message: install ? "필요한 프로그램을 설치할까요? (인터넷 사용)" : "이 작업을 빌드할까요?",
      detail: [
        install
          ? "인터넷에서 이 작업에 필요한 프로그램 꾸러미를 내려받아 작업 공간 안(node_modules)에 설치해요. 설치하면서 꾸러미의 설치 단계가 이 컴퓨터에서 실행될 수 있어요."
          : "이 작업의 코드가 이 컴퓨터에서 직접 실행돼요. 미리보기의 보호(인터넷 차단) 밖에서 도니, 믿을 수 있는 작업일 때만 진행하세요. 결과는 작업 공간 안에만 만들어요.",
        "", `실행할 명령: ${consent.command}`, ...(consent.buildScript ? [`빌드 스크립트: ${consent.buildScript}`] : []),
        `프로그램: ${consent.program}`, `위치: ${consent.folder}`,
      ].join("\n"),
      buttons: ["취소", install ? "설치" : "빌드"],
    };
  }
  return {
    message: install ? "Install the packages? (uses the internet)" : "Build this task?",
    detail: [
      install
        ? "Downloads the packages this task needs from the internet and installs them inside the workspace (node_modules). Their setup steps may run on this computer."
        : "This task's code runs directly on this computer, outside the preview's protection (no internet there). Continue only if you trust this work. The output stays inside the workspace.",
      "", `Command: ${consent.command}`, ...(consent.buildScript ? [`Build script: ${consent.buildScript}`] : []),
      `Program: ${consent.program}`, `Folder: ${consent.folder}`,
    ].join("\n"),
    buttons: ["Cancel", install ? "Install" : "Build"],
  };
}
