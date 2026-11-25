// Development-only: sign the debug sidecar with the local "Elsecrew Dev"
// code-signing identity so macOS sees every rebuild as the same program.
// A linker (ad-hoc) signature is a content hash, so each rebuild is a new
// program to the keychain and asks again before renewing the Claude login.
// With a certificate the designated requirement is the identifier plus the
// certificate, which stays the same across rebuilds.
//
// No identity (other developers, CI) or a signing failure only warns: the
// development app still runs, it just asks again. Packaged builds stage the
// release sidecar separately and are not touched here.
import { spawnSync } from "node:child_process";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

export const DEV_IDENTITY = "Elsecrew Dev";
export const SIDECAR_IDENTIFIER = "com.wonseokhan.elsecrew.sidecar";

/** Decide and perform the signing. `run(command, args)` returns
 * `{ status, stdout, stderr }`; injected so tests need no keychain. */
export function signDevSidecar({ platform, binary, run, warn, log }) {
  if (platform !== "darwin") return "skipped";
  const identities = run("/usr/bin/security", ["find-identity", "-v", "-p", "codesigning"]);
  if (identities.status !== 0 || !String(identities.stdout).includes(`"${DEV_IDENTITY}"`)) {
    warn(`[m5:dev] no valid "${DEV_IDENTITY}" code-signing identity; the sidecar stays ad-hoc signed and the keychain may ask again after rebuilds.`);
    return "no-identity";
  }
  const current = run("/usr/bin/codesign", ["-d", "-r-", binary]);
  const output = `${current.stdout ?? ""}${current.stderr ?? ""}`;
  if (current.status === 0 && output.includes(`identifier "${SIDECAR_IDENTIFIER}" and certificate leaf`)
    && run("/usr/bin/codesign", ["--verify", "--strict", binary]).status === 0) {
    const authority = run("/usr/bin/codesign", ["-dvv", binary]);
    if (`${authority.stdout ?? ""}${authority.stderr ?? ""}`.includes(`Authority=${DEV_IDENTITY}`)) return "unchanged";
  }
  const signed = run("/usr/bin/codesign", ["--force", "--sign", DEV_IDENTITY, "--identifier", SIDECAR_IDENTIFIER, binary]);
  if (signed.status !== 0) {
    warn(`[m5:dev] could not sign the sidecar with "${DEV_IDENTITY}" (exit ${String(signed.status)}); continuing unsigned.`);
    return "failed";
  }
  log(`[m5:dev] signed the sidecar with "${DEV_IDENTITY}".`);
  return "signed";
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
  signDevSidecar({
    platform: process.platform,
    binary: resolve(root, "target", "debug", "elsecrew-sidecar"),
    run: (command, args) => {
      // The first signing may wait on a keychain "allow" prompt; never hang m5:dev on it.
      const result = spawnSync(command, args, { encoding: "utf8", stdio: ["ignore", "pipe", "pipe"], timeout: 120_000 });
      return { status: result.error ? null : result.status, stdout: result.stdout ?? "", stderr: result.stderr ?? "" };
    },
    warn: (message) => console.warn(message),
    log: (message) => console.log(message),
  });
}
