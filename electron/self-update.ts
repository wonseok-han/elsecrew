// Self-hosted updater for the unsigned distribution. It implements the small
// NativeUpdater surface AppUpdateController already drives, so the menu,
// settings button and safe-quit gate stay unchanged.
//
// Trust chain: signed `latest.json` (update-manifest.ts) → package size and
// SHA-256 from that manifest → extracted bundle's identifier and version.
//
// Everything that can fail (location checks, download, extraction, writing the
// install helper) happens while checking. After the safe-quit gate allows a
// clean exit, `quitAndInstall` only starts the helper and always quits: the
// sidecar has already recorded a clean shutdown, so the app must not stay up.
// The helper waits for every process of the installed app to exit, swaps the
// app, relaunches it, and restores (and relaunches) the previous app on failure.
import { app, net } from "electron";
import { spawn, spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { EventEmitter } from "node:events";
import { accessSync, constants, createWriteStream, existsSync, mkdirSync, readdirSync, renameSync, rmSync, statfsSync, statSync, writeFileSync } from "node:fs";
import { basename, dirname, join, resolve } from "node:path";
import type { NativeUpdater } from "./app-update.ts";
import { compareVersions, MAX_FEED_BYTES, platformKey, selectUpdate, verifyFeed, type VerifiedUpdate } from "./update-manifest.ts";

const BUNDLE_ID = "com.wonseokhan.elsecrew";
const FEED_TIMEOUT_MS = 30_000;
const DOWNLOAD_TIMEOUT_MS = 30 * 60_000;

/** Ready to install: the helper is written; only spawning remains. */
interface PreparedInstall { version: string; root: string; command: string; args: string[]; env: Record<string, string> }

function powershell(): string {
  return join(process.env.SystemRoot ?? "C:\\Windows", "System32", "WindowsPowerShell", "v1.0", "powershell.exe");
}

async function readLimited(response: Response, limit: number): Promise<Buffer> {
  const reader = response.body?.getReader();
  if (!reader) throw new Error("empty response");
  const chunks: Buffer[] = [];
  let total = 0;
  for (;;) {
    const { done, value } = await reader.read();
    if (done) break;
    total += value.byteLength;
    if (total > limit) { await reader.cancel(); throw new Error("response too large"); }
    chunks.push(Buffer.from(value));
  }
  return Buffer.concat(chunks);
}

/** Streams the package to disk, enforcing the signed size and digest. */
async function download(update: VerifiedUpdate, file: string): Promise<void> {
  const response = await net.fetch(update.url, { redirect: "follow", cache: "no-store", signal: AbortSignal.timeout(DOWNLOAD_TIMEOUT_MS) });
  if (!response.ok || !response.body) throw new Error(`download failed: ${response.status}`);
  const hash = createHash("sha256");
  const out = createWriteStream(file, { flags: "wx", mode: 0o600 });
  let failure: Error | null = null;
  out.on("error", error => { failure = error; });
  let total = 0;
  try {
    const reader = response.body.getReader();
    for (;;) {
      if (failure) throw failure;
      const { done, value } = await reader.read();
      if (done) break;
      total += value.byteLength;
      if (total > update.asset.size) { await reader.cancel(); throw new Error("package larger than signed size"); }
      hash.update(value);
      if (!out.write(value)) {
        await new Promise<void>((resolveDrain, reject) => {
          out.once("drain", resolveDrain);
          out.once("error", reject);
        });
      }
    }
  } finally {
    await new Promise<void>(resolveClose => out.end(() => resolveClose()));
  }
  if (failure) throw failure;
  if (total !== update.asset.size || hash.digest("hex") !== update.asset.sha256) throw new Error("package does not match the signed manifest");
}

function plistValue(plist: string, key: string): string {
  const result = spawnSync("/usr/bin/plutil", ["-extract", key, "raw", "-o", "-", plist], { encoding: "utf8", timeout: 10_000 });
  if (result.status !== 0) throw new Error(`cannot read ${key}`);
  return result.stdout.trim();
}

/** Extracts and checks the new app; returns the path that will replace the current one. */
function extract(update: VerifiedUpdate, zip: string, root: string): string {
  const extracted = join(root, "extracted");
  mkdirSync(extracted, { recursive: true, mode: 0o700 });
  if (process.platform === "darwin") {
    const unzip = spawnSync("/usr/bin/ditto", ["-x", "-k", zip, extracted], { timeout: 300_000 });
    if (unzip.status !== 0) throw new Error("cannot extract package");
    const bundle = join(extracted, "Elsecrew.app");
    const plist = join(bundle, "Contents", "Info.plist");
    if (!existsSync(plist)) throw new Error("package has no Elsecrew.app");
    if (plistValue(plist, "CFBundleIdentifier") !== BUNDLE_ID || plistValue(plist, "CFBundleShortVersionString") !== update.version) {
      throw new Error("package identity or version does not match");
    }
    // Not an .app until installed, so LaunchServices never offers this copy.
    const payload = join(root, "payload");
    renameSync(bundle, payload);
    return payload;
  }
  const unzip = spawnSync(powershell(), ["-NoProfile", "-NonInteractive", "-Command",
    "Expand-Archive -LiteralPath $env:ELSECREW_UPDATE_ZIP -DestinationPath $env:ELSECREW_UPDATE_DIR -Force"],
  { timeout: 300_000, cwd: root, windowsHide: true, env: { ...process.env, ELSECREW_UPDATE_ZIP: zip, ELSECREW_UPDATE_DIR: extracted } });
  if (unzip.status !== 0) throw new Error("cannot extract package");
  if (existsSync(join(extracted, "Elsecrew.exe"))) return extracted;
  const folder = readdirSync(extracted).map(name => join(extracted, name))
    .find(path => statSync(path).isDirectory() && existsSync(join(path, "Elsecrew.exe")));
  if (!folder) throw new Error("package has no Elsecrew.exe");
  return folder;
}

const STAGING = /^\.elsecrew-update-(\d+\.\d+\.\d+)$/;

/**
 * Removes staging folders beside the app: every one except `keep`, or with no
 * `keep` only those for versions not newer than the running app.
 */
export function removeStaleUpdates(keep?: string): void {
  try {
    const parent = dirname(currentInstall());
    for (const name of readdirSync(parent)) {
      const match = name.match(STAGING);
      if (!match || match[1] === keep) continue;
      if (keep === undefined && compareVersions(match[1]!, app.getVersion()) > 0) continue;
      rmSync(join(parent, name), { recursive: true, force: true });
    }
  } catch { /* not replaceable here, or nothing to clean */ }
}

/**
 * The installed app to replace: the .app bundle on macOS, the install folder
 * on Windows. An unsigned app opened straight from Downloads runs from a
 * read-only App Translocation copy and cannot replace itself; it must be moved
 * (for example to Applications) first.
 */
function currentInstall(): string {
  const install = process.platform === "darwin" ? resolve(process.execPath, "..", "..", "..") : dirname(process.execPath);
  if (process.platform === "darwin" && (!install.endsWith(".app") || install.includes("/AppTranslocation/"))) {
    throw new Error("the app runs from a read-only location");
  }
  accessSync(dirname(install), constants.W_OK);
  return install;
}

// macOS helper. Waits for every process running from the installed bundle
// (the app, its helpers and the sidecar), swaps by rename on the same volume,
// relaunches, and cleans the staging folder. On any failure the previous app
// stays (or is restored) and is relaunched.
const MAC_HELPER = `#!/bin/bash
set -u
pid="$1"; current="$2"; replacement="$3"; backup="$4"; staging="$5"
# Literal (not regex) match: bundle paths may contain ( ) [ ] and spaces.
running() {
  /bin/kill -0 "$pid" 2>/dev/null && return 0
  list="$(/bin/ps -axo command=)"
  printf '%s\\n' "$list" | /usr/bin/grep -F -q -- "$current/Contents/"
}
for _ in $(seq 1 600); do running || break; /bin/sleep 0.2; done
# Something from the old app still runs: leave everything as it is. Relaunching
# here could start a second sidecar on the same data.
running && exit 1
/bin/rm -rf "$backup"
if ! /bin/mv "$current" "$backup"; then /usr/bin/open "$current"; exit 1; fi
if /bin/mv "$replacement" "$current"; then
  /usr/bin/xattr -dr com.apple.quarantine "$current" 2>/dev/null
  if /usr/bin/open "$current"; then /bin/rm -rf "$backup" "$staging"; exit 0; fi
  exit 1
fi
/bin/rm -rf "$current"
/bin/mv "$backup" "$current"
/usr/bin/open "$current"
exit 1
`;

// Windows helper, passed with -EncodedCommand (no script file, so a forced
// execution policy cannot block it) and paths through environment variables.
const WINDOWS_HELPER = `$ErrorActionPreference = 'Stop'
$id = [int]$env:ELSECREW_PID; $current = $env:ELSECREW_CURRENT; $replacement = $env:ELSECREW_REPLACEMENT; $backup = $env:ELSECREW_BACKUP; $staging = $env:ELSECREW_STAGING
$exe = Join-Path $current 'Elsecrew.exe'
Set-Location -LiteralPath ([IO.Path]::GetTempPath())
function Running { (Get-Process -Id $id -ErrorAction SilentlyContinue) -or (Get-Process -ErrorAction SilentlyContinue | Where-Object { $_.Path -and $_.Path.StartsWith($current + '\\', [StringComparison]::OrdinalIgnoreCase) }) }
function Launch { Start-Process -FilePath $exe -WorkingDirectory $current }
for ($i = 0; $i -lt 600; $i++) { if (-not (Running)) { break }; Start-Sleep -Milliseconds 200 }
# Something from the old app still runs: leave everything as it is.
if (Running) { exit 1 }
try {
  if (Test-Path -LiteralPath $backup) { Remove-Item -LiteralPath $backup -Recurse -Force }
  Rename-Item -LiteralPath $current -NewName (Split-Path -Leaf $backup)
} catch { Launch; exit 1 }
try {
  Move-Item -LiteralPath $replacement -Destination $current
  Launch
  Remove-Item -LiteralPath $backup -Recurse -Force -ErrorAction SilentlyContinue
  Remove-Item -LiteralPath $staging -Recurse -Force -ErrorAction SilentlyContinue
  exit 0
} catch {
  try {
    if (Test-Path -LiteralPath $current) { Remove-Item -LiteralPath $current -Recurse -Force -ErrorAction SilentlyContinue }
    Rename-Item -LiteralPath $backup -NewName (Split-Path -Leaf $current)
  } finally { Launch }
  exit 1
}
`;

export class SelfUpdater extends EventEmitter implements NativeUpdater {
  #feed: string | null = null;
  #prepared: PreparedInstall | null = null;
  #running = false;

  setFeedURL(options: { url: string }): void { this.#feed = options.url; }

  checkForUpdates(): void {
    if (this.#running) return;
    this.#running = true;
    void this.#check().catch(() => this.emit("error")).finally(() => { this.#running = false; });
  }

  async #check(): Promise<void> {
    const platform = platformKey(process.platform, process.arch);
    if (!this.#feed || !platform) throw new Error("updates are not supported here");
    const response = await net.fetch(this.#feed, { redirect: "follow", cache: "no-store", signal: AbortSignal.timeout(FEED_TIMEOUT_MS) });
    if (!response.ok) throw new Error(`feed unavailable: ${response.status}`);
    const manifest = verifyFeed((await readLimited(response, MAX_FEED_BYTES)).toString("utf8"));
    const update = selectUpdate(manifest, app.getVersion(), platform);
    if (!update) { this.emit("update-not-available"); return; }
    if (this.#prepared?.version === update.version && existsSync(this.#prepared.root)) { this.emit("update-downloaded"); return; }
    // Refuse before downloading when this copy of the app cannot be replaced.
    const current = currentInstall();
    // Stage next to the app so the final swap is a same-volume rename.
    const root = join(dirname(current), `.elsecrew-update-${update.version}`);
    const free = statfsSync(dirname(current));
    if (free.bavail * free.bsize < update.asset.size * 3) throw new Error("not enough free space for the update");
    this.emit("update-available");
    removeStaleUpdates(update.version);
    rmSync(root, { recursive: true, force: true });
    mkdirSync(root, { recursive: true, mode: 0o700 });
    try {
      const zip = join(root, basename(update.asset.name));
      await download(update, zip);
      const replacement = extract(update, zip, root);
      rmSync(zip, { force: true });
      this.#prepared = this.#prepare(update.version, root, current, replacement);
      this.emit("update-downloaded");
    } catch (error) {
      rmSync(root, { recursive: true, force: true });
      throw error;
    }
  }

  #prepare(version: string, root: string, current: string, replacement: string): PreparedInstall {
    const backup = `${current}.previous`;
    if (process.platform === "darwin") {
      const helper = join(root, "install.sh");
      writeFileSync(helper, MAC_HELPER, { mode: 0o700 });
      return { version, root, command: "/bin/bash", args: [helper, "", current, replacement, backup, root], env: {} };
    }
    const encoded = Buffer.from(WINDOWS_HELPER, "utf16le").toString("base64");
    return { version, root, command: powershell(), args: ["-NoProfile", "-NonInteractive", "-EncodedCommand", encoded],
      env: { ELSECREW_CURRENT: current, ELSECREW_REPLACEMENT: replacement, ELSECREW_BACKUP: backup, ELSECREW_STAGING: root } };
  }

  /** Called only after the safe-quit gate allowed a clean exit; always quits. */
  quitAndInstall(): void {
    try {
      const prepared = this.#prepared;
      if (prepared && existsSync(prepared.root)) {
        const pid = String(process.pid);
        const args = process.platform === "darwin" ? prepared.args.map((arg, index) => index === 1 ? pid : arg) : prepared.args;
        const child = spawn(prepared.command, args, {
          detached: true, stdio: "ignore", windowsHide: true, cwd: prepared.root,
          env: { ...process.env, ...prepared.env, ELSECREW_PID: pid },
        });
        child.on("error", () => undefined);
        child.unref();
      }
    } catch {
      // The previous version keeps running after relaunch; quitting is still required.
    } finally {
      app.quit();
    }
  }
}
