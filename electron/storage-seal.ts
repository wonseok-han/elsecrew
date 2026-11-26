// A clean shutdown's seal of the engine's database (user decision
// 2026-11-20): opening the app again after a normal close must not repeat the
// full ledger check, which grows with every task.
//
// At a clean exit the engine prints one stderr line naming the database file's
// SHA-256 and its own version context. This app hashes the file itself and
// signs only if they agree, so a look-alike line can only ever name the file as
// it really is. The signing key is kept encrypted with a Keychain-held key
// (Electron safeStorage) and never leaves this process. At the next start the
// seal is read, removed at once (it is used once; a crash leaves none), and
// handed to the engine only if its signature holds. The engine still checks
// that the file is exactly what was sealed before it skips anything.
//
// A seal vouches for the file as the app left it, including what the app
// itself wrote in that session, which no full check has seen (review I7).
// The engine checks a copy in full while the app rests (user decision
// 2026-11-20) and reports when that state was taken; the seal carries the
// latest such time. Only if no full check succeeded for FULL_CHECK_INTERVAL_MS
// (the app was never left resting) does the next start check in full again.
import { createHash, createHmac, randomBytes, timingSafeEqual } from "node:crypto";
import { createReadStream, mkdirSync, readFileSync, renameSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";

/** `<64 hex sha256 of the file>.<engine context>`, as the engine accepts it. */
const SEAL_TOKEN = /^([0-9a-f]{64})\.([0-9a-z.+-]{1,64})$/;
const SEAL_LINE = /^elsecrew-storage-seal v1 (\S+)(?: full=(\d{1,16}))?$/;
const SEAL_FILE = "storage-seal-v1.json";
/** How long seals may stand in for the full check before one runs again. */
export const FULL_CHECK_INTERVAL_MS = 7 * 24 * 60 * 60 * 1000;
const KEY_FILE = "storage-seal-key-v1";

export interface SealKeyProtection {
  available(): boolean;
  encrypt(text: string): Buffer;
  decrypt(data: Buffer): string;
}

export function validSealToken(token: string): boolean {
  return SEAL_TOKEN.test(token);
}

/** The engine's seal line: only the last stderr line counts, and only when
 * it is that line exactly (nothing printed after it). `fullCheckAt`: when the
 * state its background full check proved was taken, if it ran one. */
export function sealFromStderr(stderr: string): { token: string; fullCheckAt: number | null } | null {
  const lines = stderr.split("\n").map(line => line.trimEnd()).filter(line => line.length > 0);
  const last = lines.at(-1);
  const match = last ? SEAL_LINE.exec(last) : null;
  if (!match || !validSealToken(match[1]!)) return null;
  const fullCheckAt = match[2] === undefined ? null : Number(match[2]);
  return { token: match[1]!, fullCheckAt: fullCheckAt !== null && Number.isSafeInteger(fullCheckAt) ? fullCheckAt : null };
}

export async function sha256File(path: string): Promise<string | null> {
  try {
    const hash = createHash("sha256");
    await new Promise<void>((resolveRead, rejectRead) => {
      const stream = createReadStream(path);
      stream.on("data", chunk => hash.update(chunk));
      stream.once("error", rejectRead);
      stream.once("end", () => resolveRead());
    });
    return hash.digest("hex");
  } catch {
    return null;
  }
}

export class StorageSealStore {
  readonly #directory: string;
  readonly #dataDirectory: string;
  readonly #appVersion: string;
  readonly #protection: SealKeyProtection;

  /** `directory` holds the seal and the encrypted key; `dataDirectory` is the
   * engine's, whose database the seal is about. */
  constructor(directory: string, dataDirectory: string, appVersion: string, protection: SealKeyProtection) {
    this.#directory = directory;
    this.#dataDirectory = dataDirectory;
    this.#appVersion = appVersion;
    this.#protection = protection;
  }

  /** The signing key, made once. Null when the Keychain cannot protect it:
   * then there are no seals and every start checks in full. */
  #key(create: boolean): Buffer | null {
    try {
      if (!this.#protection.available()) return null;
      const file = join(this.#directory, KEY_FILE);
      try {
        const key = Buffer.from(this.#protection.decrypt(readFileSync(file)), "hex");
        return key.length === 32 ? key : null;
      } catch {
        if (!create) return null;
        const key = randomBytes(32);
        mkdirSync(this.#directory, { recursive: true, mode: 0o700 });
        writeFileSync(file, this.#protection.encrypt(key.toString("hex")), { mode: 0o600 });
        return key;
      }
    } catch {
      return null;
    }
  }

  #mac(key: Buffer, token: string, fullCheckAt: number): Buffer {
    return createHmac("sha256", key)
      .update(`elsecrew-storage-seal-v1\n${this.#appVersion}\n${this.#dataDirectory}\n${fullCheckAt}\n${token}`)
      .digest();
  }

  /** Read once and remove: the token to hand the engine and when the last
   * full check ran, or null (then this start checks in full). */
  consume(now = Date.now()): { token: string; fullCheckAt: number } | null {
    const file = join(this.#directory, SEAL_FILE);
    let text: string;
    try { text = readFileSync(file, "utf8"); } catch { return null; }
    try { rmSync(file, { force: true }); } catch { return null; }
    try {
      const seal = JSON.parse(text) as unknown;
      if (!seal || typeof seal !== "object") return null;
      const { v, token, mac, fullCheckAt } = seal as Record<string, unknown>;
      if (v !== 1 || typeof token !== "string" || typeof mac !== "string" || !validSealToken(token)
        || typeof fullCheckAt !== "number" || !Number.isSafeInteger(fullCheckAt)) return null;
      const key = this.#key(false);
      if (!key) return null;
      const given = Buffer.from(mac, "hex");
      const expected = this.#mac(key, token, fullCheckAt);
      if (given.length !== expected.length || !timingSafeEqual(given, expected)) return null;
      // Too long since a full check (or a clock that went back): check in full.
      if (fullCheckAt > now || now - fullCheckAt > FULL_CHECK_INTERVAL_MS) return null;
      return { token, fullCheckAt };
    } catch {
      return null;
    }
  }

  /** After a clean exit: sign the engine's token if the database file really
   * has that hash. `fullCheckAt`: when the last full check ran (this
   * session's start, unless it started from a seal). */
  async seal(token: string, databasePath: string, fullCheckAt: number): Promise<boolean> {
    const match = SEAL_TOKEN.exec(token);
    if (!match) return false;
    const actual = await sha256File(databasePath);
    if (actual !== match[1]) return false;
    const key = this.#key(true);
    if (!key) return false;
    try {
      const file = join(this.#directory, SEAL_FILE);
      const temporary = `${file}.${randomBytes(8).toString("hex")}.tmp`;
      mkdirSync(dirname(file), { recursive: true, mode: 0o700 });
      writeFileSync(temporary, JSON.stringify({ v: 1, token, fullCheckAt, mac: this.#mac(key, token, fullCheckAt).toString("hex") }), { mode: 0o600 });
      renameSync(temporary, file);
      return true;
    } catch {
      return false;
    }
  }
}
