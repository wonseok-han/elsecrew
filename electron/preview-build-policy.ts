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
  /** This job's own temporary folder (its TMPDIR). */
  temp: string;
  /** The package manager's caches, exact folders (`previewBuildCaches`). */
  caches: readonly string[];
  /** Never read nor written (`previewBuildHiddenPaths`, plus real targets). */
  hidden: readonly string[];
}

/** The one package manager's cache folders a job may write (re-review B2):
 * not `~/Library/Caches` or `~/.cache` as a whole. */
export function previewBuildCaches(manager: PreviewPackageManagerV1, yarnBerry: boolean, home: string): string[] {
  switch (manager) {
    case "npm": return [`${home}/.npm/_cacache`, `${home}/.npm/_logs`];
    case "pnpm": return [`${home}/Library/pnpm/store`, `${home}/Library/Caches/pnpm`];
    case "yarn": return yarnBerry ? [`${home}/.yarn/berry/cache`] : [`${home}/Library/Caches/Yarn`];
  }
}

/** Key, sign-in and app places a job may never read nor write (re-review
 * B2). The host adds each one's real target when it leads elsewhere. */
export function previewBuildHiddenPaths(home: string, appData: readonly string[]): string[] {
  return [
    ...appData,
    `${home}/.ssh`, `${home}/.aws`, `${home}/.config/gh`, `${home}/.config/gcloud`, `${home}/.azure`, `${home}/.kube`,
    `${home}/.docker`, `${home}/.gnupg`, `${home}/.netrc`, `${home}/.git-credentials`, `${home}/.config/git/credentials`,
    `${home}/.password-store`, `${home}/Library/Keychains`, `${home}/.claude`, `${home}/.codex`,
  ];
}

/**
 * The macOS sandbox (`sandbox-exec`) a build or an installation runs in
 * (PR #22 review B2, re-review B2). Writing: only this workspace, this
 * job's own temporary folder and the package manager's exact cache folders
 * (a missing parent such as `~/.npm` may be created, nothing else in it).
 * Reading: everything the person can read, except the hidden places.
 * Network: only for installing. Everything the job starts inherits it.
 */
export function previewBuildSandboxProfile(step: PreviewBuildStepV1, places: PreviewSandboxPlacesV1): string {
  const writable = [places.workspace, places.temp, ...places.caches];
  // A cache's missing parents below the home folder (`~/.npm`, `~/.yarn/berry`) may be created; nothing else in them.
  const parents = [...new Set(places.caches.flatMap(cache => {
    const parts = cache.split("/");
    return parts.slice(3, -1).map((_, index) => parts.slice(0, index + 4).join("/"));
  }))];
  return [
    "(version 1)",
    "(allow default)",
    "(deny file-write*)",
    `(allow file-write* ${writable.map(path => `(subpath ${profilePath(path)})`).join(" ")} (literal "/dev/null") (literal "/dev/zero") (regex #"^/dev/tty") (subpath "/dev/fd"))`,
    ...(parents.length ? [`(allow file-write-create ${parents.map(path => `(literal ${profilePath(path)})`).join(" ")})`] : []),
    `(deny file-read* file-write* ${places.hidden.map(path => `(subpath ${profilePath(path)})`).join(" ")})`,
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
const BEGIN_MARK = "-----BEGIN ";
const END_MARK = "-----END ";
/** What a line too long to keep says about a block, from its whole text. */
const LONG_PLAIN = "\u0000long";
const LONG_OPENS = "\u0000long-opens-block";
const LONG_CLOSES = "\u0000long-closes-block";
/** Key material with no name: a long unbroken run of base64 with upper,
 * lower case and digits, and at most two slashes (a path has more; a
 * lowercase commit hash is not a key). */
function hasKeyMaterial(line: string): boolean {
  return (line.match(/[A-Za-z0-9+/=_-]{40,}/g) ?? []).some(run =>
    /[A-Z]/.test(run) && /[a-z]/.test(run) && /[0-9]/.test(run) && (run.match(/\//g)?.length ?? 0) <= 2);
}

/**
 * One output stream's lines as they are kept and shown (PR #22 review B4,
 * re-review B4; the PR #11 rule, fail closed). Stateful: once a block
 * begins (`-----BEGIN `), every line is hidden until a line ends it
 * (`-----END `) or the stream ends — however long the lines are, however
 * they were split: the last marker on a line decides. A line with nameless
 * key material, or one too long to keep whole, is hidden rather than cut.
 * Everything else is masked with `maskSecrets`.
 */
export class PreviewLogMasker {
  #inBlock = false;
  /** Whether a block is open now (tests). */
  get inBlock(): boolean { return this.#inBlock; }
  line(raw: string): string {
    if (raw === LONG_OPENS) { this.#inBlock = true; return PREVIEW_LOG_HIDDEN; }
    if (raw === LONG_CLOSES) { this.#inBlock = false; return PREVIEW_LOG_HIDDEN; }
    if (raw === LONG_PLAIN) return PREVIEW_LOG_HIDDEN;
    const plain = raw.replace(ANSI, "");
    const begin = plain.lastIndexOf(BEGIN_MARK);
    const end = plain.lastIndexOf(END_MARK);
    if (this.#inBlock || begin >= 0) {
      if (begin >= 0 || end >= 0) this.#inBlock = begin > end;
      return PREVIEW_LOG_HIDDEN;
    }
    const frames = plain.split("\r");
    const line = (frames.reverse().find(frame => frame.trim().length > 0) ?? "").replace(CONTROL, "").replace(/\t/g, "  ");
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
 * grows past the bound is never kept or cut (a secret could straddle the
 * cut): it is read to its end only to learn whether it leaves a block open,
 * and becomes one hidden line that tells the masker so (re-review B4). */
export class PreviewLineSplitter {
  #pending = "";
  #long: { last: "begin" | "end" | null; carry: string } | null = null;
  #scan(part: string): void {
    const long = this.#long!;
    const text = long.carry + part;
    const begin = text.lastIndexOf(BEGIN_MARK);
    const end = text.lastIndexOf(END_MARK);
    if (begin >= 0 || end >= 0) long.last = begin > end ? "begin" : "end";
    long.carry = text.slice(-16);
  }
  #marker(): string {
    const last = this.#long?.last ?? null;
    this.#long = null;
    return last === "begin" ? LONG_OPENS : last === "end" ? LONG_CLOSES : LONG_PLAIN;
  }
  push(chunk: string): string[] {
    const out: string[] = [];
    let text = chunk;
    while (text.length > 0) {
      const newline = text.indexOf("\n");
      if (this.#long) {
        this.#scan(newline < 0 ? text : text.slice(0, newline));
        if (newline < 0) return out;
        out.push(this.#marker());
        text = text.slice(newline + 1);
        continue;
      }
      if (newline < 0) {
        this.#pending += text;
        if (this.#pending.length > PREVIEW_BUILD_PENDING_MAX_CHARS) {
          this.#long = { last: null, carry: "" };
          this.#scan(this.#pending);
          this.#pending = "";
        }
        return out;
      }
      const line = this.#pending + text.slice(0, newline);
      this.#pending = "";
      text = text.slice(newline + 1);
      if (line.length > PREVIEW_BUILD_PENDING_MAX_CHARS) {
        this.#long = { last: null, carry: "" };
        this.#scan(line);
        out.push(this.#marker());
      } else out.push(line);
    }
    return out;
  }
  flush(): string[] {
    if (this.#long) return [this.#marker()];
    const rest = this.#pending;
    this.#pending = "";
    return rest.length > 0 ? [rest] : [];
  }
}

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
  /** The folder's device:inode (with `folder`). */
  folderIdentity: string;
  /** Every input file and its SHA-256: what the fingerprint stands for. */
  inputs: { name: string; sha256: string }[];
  /** The person's own package manager settings files that apply. */
  userConfigs: string[];
  /** Where it may write besides the workspace and its own temporary folder. */
  caches: string[];
  /** What it may never read nor write. */
  hidden: string[];
  /** Asked again: what was agreed changed before it ran. */
  changed: boolean;
}
export function previewBuildConsentBox(consent: PreviewBuildConsentV1, korean: boolean): PreviewBuildBoxV1 {
  const install = consent.step === "install";
  const line = (text: string) => previewBuildLine(text);
  const more = consent.dependencyCount - consent.dependencies.length;
  // Every input the fingerprint stands for, by name and hash (re-review B1).
  const inputs = consent.inputs.map(input => `${input.name} (${input.sha256.slice(0, 16)})`).join(", ");
  const list = (items: readonly string[]) => items.join(", ");
  if (korean) {
    const packages = `${consent.dependencies.join(", ")}${more > 0 ? ` 외 ${more}개` : ""}`;
    return {
      message: `${consent.changed ? "내용이 바뀌어 다시 확인해요. " : ""}${install ? "필요한 프로그램을 설치할까요? (인터넷 사용)" : "이 작업을 빌드할까요?"}`,
      detail: [
        install
          ? "인터넷에서 아래 꾸러미(와 그것들이 쓰는 꾸러미)를 내려받아 작업 공간 안(node_modules)에 설치해요. 꾸러미의 설치 스크립트는 실행하지 않아요."
          : "이 작업의 코드가 내 컴퓨터에서 실행돼요(미리보기의 보호 화면 밖). 믿을 수 있는 작업일 때만 진행하세요.",
        `쓸 수 있는 곳: 이 작업 공간, 이 ${install ? "설치" : "빌드"} 전용 임시 폴더, ${list(consent.caches)}.`,
        `읽지도 쓰지도 못하는 곳: ${list(consent.hidden)}.`,
        `그 밖의 내 파일은 읽을 수 있어요.${install ? "" : " 빌드 중에는 인터넷을 쓸 수 없어요."}`,
        "", `실행할 명령: ${consent.command}`,
        ...(consent.buildScript ? [`빌드 스크립트: ${line(consent.buildScript)}`] : []),
        ...consent.hooks.map(hook => `함께 실행: ${hook.name} — ${line(hook.script)}`),
        ...(install ? [`꾸러미 ${consent.dependencyCount}개: ${packages}`] : []),
        `확인한 파일: ${inputs}`,
        ...(consent.userConfigs.length ? [`적용되는 내 설정: ${list(consent.userConfigs)}`] : []),
        `프로그램: ${consent.program}`, `위치: ${consent.folder} (폴더 ${consent.folderIdentity})`,
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
      `It can write only to: this workspace, a temporary folder of this ${install ? "installation" : "build"} only, ${list(consent.caches)}.`,
      `It can neither read nor write: ${list(consent.hidden)}.`,
      `It can read your other files.${install ? "" : " A build cannot use the internet."}`,
      "", `Command: ${consent.command}`,
      ...(consent.buildScript ? [`Build script: ${line(consent.buildScript)}`] : []),
      ...consent.hooks.map(hook => `Also runs: ${hook.name} — ${line(hook.script)}`),
      ...(install ? [`${consent.dependencyCount} packages: ${packages}`] : []),
      `Files checked: ${inputs}`,
      ...(consent.userConfigs.length ? [`Your settings that apply: ${list(consent.userConfigs)}`] : []),
      `Program: ${consent.program}`, `Folder: ${consent.folder} (folder ${consent.folderIdentity})`,
    ].join("\n"),
    buttons: ["Cancel", install ? "Install" : "Build"],
  };
}
