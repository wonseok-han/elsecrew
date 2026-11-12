import { lstat, open, realpath, rename, unlink } from "node:fs/promises";
import { randomUUID } from "node:crypto";
import { join, win32 } from "node:path";
import type { GitSelection, GitSelectionAction } from "../src/m5/contracts/git-selection-v1.ts";

const MAX_RECORD = 65536;
const status = (state: GitSelection["state"], path: string | null = null): GitSelection => ({ state, path, executionEnabled: false });

/** Advisory user preference only. Never a trusted executable or launch grant.
 * The path comes exclusively from a main-owned native dialog; Rust must rebind
 * and validate the installation/dependencies before any future invocation.
 */
export class GitSelectionStore {
  #queue: Promise<void> = Promise.resolve();
  constructor(private readonly dataRoot: string, private readonly platform = process.platform,
    private readonly legacyRoot?: string) {}

  request(action: GitSelectionAction, choose: () => Promise<string | null>): Promise<GitSelection> {
    if (this.platform !== "win32") return Promise.resolve(status("unsupported"));
    // Migration, selection and inspection share one owner; an old preference
    // must never publish over a newer native-dialog selection.
    const task = this.#queue.then(async () => {
      if (action === "inspect") await this.#migratePreference();
      if (action === "choose") {
        const path = await choose();
        if (path !== null) await this.#write(await this.#checkCandidate(path));
      } else if (action === "forget") {
        await this.#write(null); // Never remove Git itself.
      }
      return this.#inspect();
    });
    this.#queue = task.then(() => {}, () => {});
    return task;
  }

  async #migratePreference(): Promise<void> {
    if (!this.legacyRoot) return;
    try { await lstat(join(await this.#root(), "git-selection-v1.json")); return; }
    catch (error) { if ((error as NodeJS.ErrnoException).code !== "ENOENT") throw error; }
    const old = await new GitSelectionStore(this.legacyRoot, this.platform).request("inspect", async () => null);
    if (old.state === "selected") await this.#write(old.path);
    else if (old.state === "unavailable") throw new Error("previous Git preference is unavailable");
    // Preserve the old preference file. A new null record prevents resurrection
    // after the user explicitly forgets the migrated selection.
  }

  async #root(): Promise<string> {
    const info = await lstat(this.dataRoot);
    if (!info.isDirectory() || info.isSymbolicLink()) throw new Error("invalid Git settings directory");
    return realpath(this.dataRoot);
  }
  async #checkCandidate(path: string): Promise<string> {
    if (!win32.isAbsolute(path) || path.length > 32767 || /[\u0000-\u001f]/.test(path)
      || win32.basename(path).toLowerCase() !== "git.exe") throw new Error("select git.exe");
    const info = await lstat(path);
    if (!info.isFile() || info.isSymbolicLink()) throw new Error("Git selection is not a regular file");
    const canonical = await realpath(path);
    if (win32.basename(canonical).toLowerCase() !== "git.exe") throw new Error("invalid resolved Git selection");
    return canonical;
  }
  async #inspect(): Promise<GitSelection> {
    try {
      const path = join(await this.#root(), "git-selection-v1.json");
      let info;
      try { info = await lstat(path); }
      catch (error) {
        if ((error as NodeJS.ErrnoException).code === "ENOENT") return status("unselected");
        throw error;
      }
      if (!info.isFile() || info.isSymbolicLink() || info.size > MAX_RECORD) throw new Error("invalid Git preference");
      const file = await open(path, "r");
      try {
        const held = await file.stat();
        if (!held.isFile() || held.dev !== info.dev || held.ino !== info.ino || held.size > MAX_RECORD) throw new Error("Git preference changed");
        const bytes = Buffer.alloc(MAX_RECORD + 1);
        let length = 0;
        while (length < bytes.length) {
          const read = await file.read(bytes, length, bytes.length - length, length);
          if (read.bytesRead === 0) break;
          length += read.bytesRead;
        }
        if (length > MAX_RECORD) throw new Error("Git preference too large");
        const record = JSON.parse(bytes.subarray(0, length).toString("utf8"));
        if (!record || Object.keys(record).sort().join() !== "path,schemaVersion" || record.schemaVersion !== 1
          || (record.path !== null && typeof record.path !== "string")) throw new Error("invalid Git preference");
        if (record.path === null) return status("unselected");
        return status("selected", await this.#checkCandidate(record.path));
      } finally { await file.close(); }
    } catch { return status("unavailable"); }
  }
  async #write(path: string | null): Promise<void> {
    const root = await this.#root();
    const temporary = join(root, `git-selection-${randomUUID()}.tmp`);
    const file = await open(temporary, "wx", 0o600);
    try {
      await file.writeFile(JSON.stringify({ schemaVersion: 1, path }));
      await file.sync();
    } catch (error) {
      await file.close();
      await unlink(temporary).catch(() => {});
      throw error;
    }
    await file.close();
    try {
      if (await this.#root() !== root) throw new Error("Git settings directory changed");
      await rename(temporary, join(root, "git-selection-v1.json"));
    } catch (error) {
      await unlink(temporary).catch(() => {});
      throw error;
    }
  }
}
