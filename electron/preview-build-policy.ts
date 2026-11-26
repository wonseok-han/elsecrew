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

/** After the leader ended or was killed, how long its pipes may stay open
 * (held by a descendant) before the job ends without them. */
export const PREVIEW_BUILD_PIPE_GRACE_MS = 2_000;
/** How often a running job's descendants are looked for. */
export const PREVIEW_BUILD_TRACK_MS = 1_000;

/** The fixed arguments of one step. A lockfile is never rewritten: with one
 * the installation is frozen to it; without one none is created (npm, pnpm).
 * Installing never runs the packages' own install scripts (PR #22 review
 * B1·B2): what runs is what the box named. Yarn 2+ skips them with its own flag. */
export function previewBuildArgs(manager: PreviewPackageManagerV1, step: PreviewBuildStepV1, lockfile: boolean, yarnBerry = false): string[] {
  if (step === "build") return ["run", "build"];
  switch (manager) {
    case "npm": return lockfile ? ["ci", "--ignore-scripts", "--no-audit", "--no-fund"] : ["install", "--ignore-scripts", "--no-package-lock", "--no-audit", "--no-fund"];
    case "pnpm": return lockfile ? ["install", "--frozen-lockfile", "--ignore-scripts"] : ["install", "--lockfile=false", "--ignore-scripts"];
    case "yarn": return yarnBerry
      ? (lockfile ? ["install", "--immutable", "--mode=skip-build"] : ["install", "--mode=skip-build"])
      : (lockfile ? ["install", "--frozen-lockfile", "--ignore-scripts"] : ["install", "--ignore-scripts"]);
  }
}

/** How the command reads in the consent box and the tab. */
export function previewBuildCommandLine(manager: PreviewPackageManagerV1, step: PreviewBuildStepV1, lockfile: boolean, yarnBerry = false): string {
  return [manager, ...previewBuildArgs(manager, step, lockfile, yarnBerry)].join(" ");
}

/** A path the sandbox profile may name: absolute, nothing that could end
 * or bend the profile's string. */
function profilePath(path: string): string {
  if (!path.startsWith("/") || /["\\\u0000-\u001f\u007f]/u.test(path)) throw new Error("path cannot go into the sandbox profile");
  return `"${path}"`;
}

export interface PreviewSandboxPlacesV1 {
  workspace: string;
  home: string;
  /** The real folder above the user's temporary folder (`/private/var/folders/…/…`). */
  tempRoot: string;
  /** Never read nor written: the app's own data, keys. */
  hidden: readonly string[];
}

/**
 * The macOS sandbox (`sandbox-exec`) a build or an installation runs in
 * (PR #22 review B2). Writing: only the workspace, the temporary folders and
 * the package managers' caches. Reading: everything the person can read,
 * except the app's own data and key folders. Network: only for installing.
 * Everything the job starts inherits it.
 */
export function previewBuildSandboxProfile(step: PreviewBuildStepV1, places: PreviewSandboxPlacesV1): string {
  const home = places.home;
  const writable = [
    places.workspace, places.tempRoot, "/private/tmp",
    `${home}/.npm`, `${home}/Library/Caches`, `${home}/.cache`, `${home}/Library/pnpm`, `${home}/.local/share/pnpm`,
    `${home}/.pnpm-store`, `${home}/.yarn`,
  ];
  const hidden = [...places.hidden, `${home}/.ssh`, `${home}/.aws`, `${home}/.gnupg`, `${home}/Library/Keychains`];
  return [
    "(version 1)",
    "(allow default)",
    "(deny file-write*)",
    `(allow file-write* ${writable.map(path => `(subpath ${profilePath(path)})`).join(" ")} (literal "/dev/null") (literal "/dev/zero") (regex #"^/dev/tty") (subpath "/dev/fd"))`,
    `(deny file-read* file-write* ${hidden.map(path => `(subpath ${profilePath(path)})`).join(" ")})`,
    ...(step === "build" ? ["(deny network*)"] : []),
  ].join("\n");
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
/** What a hidden line becomes. */
export const PREVIEW_LOG_HIDDEN = "(가림)";
const BLOCK_BEGIN = /-----BEGIN [A-Z0-9 ]+-----/;
const BLOCK_END = /-----END [A-Z0-9 ]+-----/;
/** Key material with no name: a long unbroken run of base64 with upper,
 * lower case and digits, and at most two slashes (a path has more; a
 * lowercase commit hash is not a key). */
function hasKeyMaterial(line: string): boolean {
  return (line.match(/[A-Za-z0-9+/=_-]{40,}/g) ?? []).some(run =>
    /[A-Z]/.test(run) && /[a-z]/.test(run) && /[0-9]/.test(run) && (run.match(/\//g)?.length ?? 0) <= 2);
}

/**
 * One output stream's lines as they are kept and shown (PR #22 review B4,
 * the PR #11 rule, fail closed). Stateful: a PEM block (`-----BEGIN … -----`
 * to `-----END … -----`) is hidden whole, every line of it, however it is
 * split. A line with nameless key material, or one too long to keep whole,
 * is hidden rather than cut. Everything else is masked with `maskSecrets`.
 */
export class PreviewLogMasker {
  #inBlock = false;
  line(raw: string): string {
    const frames = raw.replace(ANSI, "").split("\r");
    const line = (frames.reverse().find(frame => frame.trim().length > 0) ?? "").replace(CONTROL, "").replace(/\t/g, "  ");
    if (this.#inBlock) {
      if (BLOCK_END.test(line)) this.#inBlock = false;
      return PREVIEW_LOG_HIDDEN;
    }
    if (BLOCK_BEGIN.test(line)) {
      this.#inBlock = !BLOCK_END.test(line.slice(line.search(BLOCK_BEGIN) + 10));
      return PREVIEW_LOG_HIDDEN;
    }
    // An escaped block inside one line (a JSON key file's "-----BEGIN…\n…").
    if (/BEGIN [A-Z ]*(?:PRIVATE )?KEY/.test(line) || hasKeyMaterial(line) || line.length > PREVIEW_BUILD_LINE_MAX_CHARS_V1) return PREVIEW_LOG_HIDDEN;
    return maskSecrets(line);
  }
}

/** One line on its own (no block state): tests and one-off text such as the
 * build script shown in the consent box. */
export function previewBuildLine(raw: string): string {
  return new PreviewLogMasker().line(raw);
}

/** Splits a stream into whole lines; the rest waits for more. A line that
 * grows past the bound is never cut in two (a secret could straddle the
 * cut): it becomes one hidden line and the rest of it is dropped. */
export class PreviewLineSplitter {
  #pending = "";
  #dropping = false;
  push(chunk: string): string[] {
    const out: string[] = [];
    let text = chunk;
    if (this.#dropping) {
      const newline = text.indexOf("\n");
      if (newline < 0) return out;
      this.#dropping = false;
      text = text.slice(newline + 1);
    }
    this.#pending += text;
    const parts = this.#pending.split("\n");
    this.#pending = parts.pop() ?? "";
    out.push(...parts.map(part => part.length > PREVIEW_BUILD_PENDING_MAX_CHARS ? PREVIEW_LOG_HIDDEN_LONG : part));
    if (this.#pending.length > PREVIEW_BUILD_PENDING_MAX_CHARS) {
      out.push(PREVIEW_LOG_HIDDEN_LONG);
      this.#pending = "";
      this.#dropping = true;
    }
    return out;
  }
  flush(): string[] {
    const rest = this.#pending;
    this.#pending = "";
    return rest.length > 0 ? [rest] : [];
  }
}
/** Marks a line too long to keep (it reaches the masker as itself: hidden). */
const PREVIEW_LOG_HIDDEN_LONG = "x".repeat(PREVIEW_BUILD_LINE_MAX_CHARS_V1 + 1);

/** The kept log's file name: no project or chat name in it. */
export function previewBuildLogName(projectId: string, chatId: string): string {
  return `${createHash("sha256").update(`${projectId}\u0000${chatId}`).digest("hex").slice(0, 32)}.log`;
}

/** The words of main's consent box (PR #12·#14: main asks, never the page).
 * What the box says is what runs (PR #22 review B1·B2): the command, the
 * scripts npm runs around it, the packages, the program and the folder, and
 * — plainly — what the sandbox does and does not keep it from. */
export interface PreviewBuildBoxV1 { message: string; detail: string; buttons: string[] }
export interface PreviewBuildConsentV1 {
  step: PreviewBuildStepV1;
  command: string;
  program: string;
  folder: string;
  buildScript: string | null;
  hooks: { name: string; script: string }[];
  dependencies: string[];
  dependencyCount: number;
  /** Asked again: what was agreed changed before it ran. */
  changed: boolean;
}
export function previewBuildConsentBox(consent: PreviewBuildConsentV1, korean: boolean): PreviewBuildBoxV1 {
  const install = consent.step === "install";
  const line = (text: string) => previewBuildLine(text);
  const more = consent.dependencyCount - consent.dependencies.length;
  if (korean) {
    const packages = `${consent.dependencies.join(", ")}${more > 0 ? ` 외 ${more}개` : ""}`;
    return {
      message: `${consent.changed ? "내용이 바뀌어 다시 확인해요. " : ""}${install ? "필요한 프로그램을 설치할까요? (인터넷 사용)" : "이 작업을 빌드할까요?"}`,
      detail: [
        install
          ? `인터넷에서 아래 꾸러미(와 그것들이 쓰는 꾸러미)를 내려받아 작업 공간 안(node_modules)에 설치해요. 꾸러미의 설치 스크립트는 실행하지 않아요.`
          : "이 작업의 코드가 내 컴퓨터에서 실행돼요(미리보기의 보호 화면 밖). 믿을 수 있는 작업일 때만 진행하세요.",
        "실행 중에는 작업 공간·임시 폴더·꾸러미 캐시에만 쓸 수 있고, 앱 데이터와 키 폴더(~/.ssh 등)는 읽을 수 없어요. 그 밖의 내 파일은 읽을 수 있어요." + (install ? "" : " 빌드 중에는 인터넷을 쓸 수 없어요."),
        "", `실행할 명령: ${consent.command}`,
        ...(consent.buildScript ? [`빌드 스크립트: ${line(consent.buildScript)}`] : []),
        ...consent.hooks.map(hook => `함께 실행: ${hook.name} — ${line(hook.script)}`),
        ...(install ? [`꾸러미 ${consent.dependencyCount}개: ${packages}`] : []),
        `프로그램: ${consent.program}`, `위치: ${consent.folder}`,
      ].join("\n"),
      buttons: ["취소", install ? "설치" : "빌드"],
    };
  }
  const packages = `${consent.dependencies.join(", ")}${more > 0 ? ` and ${more} more` : ""}`;
  return {
    message: `${consent.changed ? "This changed; please check again. " : ""}${install ? "Install the packages? (uses the internet)" : "Build this task?"}`,
    detail: [
      install
        ? "Downloads the packages below (and those they use) from the internet into the workspace (node_modules). Their install scripts are not run."
        : "This task's code runs on your computer (outside the preview's protection). Continue only if you trust this work.",
      "While it runs it can write only to the workspace, temporary folders and package caches, and cannot read the app's data or key folders (~/.ssh and others). It can read your other files." + (install ? "" : " A build cannot use the internet."),
      "", `Command: ${consent.command}`,
      ...(consent.buildScript ? [`Build script: ${line(consent.buildScript)}`] : []),
      ...consent.hooks.map(hook => `Also runs: ${hook.name} — ${line(hook.script)}`),
      ...(install ? [`${consent.dependencyCount} packages: ${packages}`] : []),
      `Program: ${consent.program}`, `Folder: ${consent.folder}`,
    ].join("\n"),
    buttons: ["Cancel", install ? "Install" : "Build"],
  };
}
