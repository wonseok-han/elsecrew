import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { existsSync, mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import { FULL_CHECK_INTERVAL_MS, sealFromStderr, StorageSealStore, validSealToken, type SealKeyProtection } from "../electron/storage-seal.ts";

/** Stands in for safeStorage: reversible, and can be switched off. */
function protection(available = true): SealKeyProtection {
  return {
    available: () => available,
    encrypt: text => Buffer.from(`enc:${text}`),
    decrypt: data => { const text = data.toString(); if (!text.startsWith("enc:")) throw new Error("not ours"); return text.slice(4); },
  };
}

function fixture() {
  const root = mkdtempSync(join(tmpdir(), "elsecrew-seal-"));
  const database = join(root, "elsecrew.sqlite3");
  writeFileSync(database, "database bytes");
  const hash = createHash("sha256").update("database bytes").digest("hex");
  return { root, database, token: `${hash}.0.1.0+s13` };
}

test("only the engine's last stderr line is a seal, and only in its exact form", () => {
  const token = `${"a".repeat(64)}.0.1.0+s13`;
  assert.equal(sealFromStderr(`storage admission: clean shutdown seal\nelsecrew-storage-seal v1 ${token}\n`), token);
  assert.equal(sealFromStderr(`elsecrew-storage-seal v1 ${token}\nsomething after\n`), null);
  assert.equal(sealFromStderr(`elsecrew-storage-seal v1 ${"A".repeat(64)}.x\n`), null);
  assert.equal(sealFromStderr(`elsecrew-storage-seal v1 ${"a".repeat(64)}\n`), null);
  assert.equal(sealFromStderr(""), null);
  assert.equal(validSealToken(token), true);
  assert.equal(validSealToken(`${"a".repeat(64)}.UPPER`), false);
});

test("a seal is signed only for the file as it really is, and is used once", async () => {
  const { root, database, token } = fixture();
  const store = new StorageSealStore(root, "/data", "1.0.0", protection());
  // A token naming other bytes than the file's is never signed.
  const checkedAt = Date.now() - 1000;
  assert.equal(await store.seal(`${"b".repeat(64)}.0.1.0+s13`, database, checkedAt), false);
  assert.equal(store.consume(), null);
  assert.equal(await store.seal(token, database, checkedAt), true);
  assert.deepEqual(store.consume(), { token, fullCheckAt: checkedAt });
  assert.equal(existsSync(join(root, "storage-seal-v1.json")), false, "removed when read");
  assert.equal(store.consume(), null, "used once");
});

test("a changed seal, another app version or data folder, or no Keychain gives no seal", async () => {
  const { root, database, token } = fixture();
  const store = new StorageSealStore(root, "/data", "1.0.0", protection());
  const sealFile = join(root, "storage-seal-v1.json");

  const now = Date.now();
  assert.equal(await store.seal(token, database, now), true);
  const written = JSON.parse(readFileSync(sealFile, "utf8"));
  writeFileSync(sealFile, JSON.stringify({ ...written, token: `${"c".repeat(64)}.0.1.0+s13` }));
  assert.equal(store.consume(), null, "token swapped under the signature");

  assert.equal(await store.seal(token, database, now), true);
  writeFileSync(sealFile, JSON.stringify({ ...JSON.parse(readFileSync(sealFile, "utf8")), fullCheckAt: now - 5 }));
  assert.equal(store.consume(), null, "the full-check time is signed too");

  for (const other of [
    new StorageSealStore(root, "/data", "2.0.0", protection()),
    new StorageSealStore(root, "/other", "1.0.0", protection()),
  ]) {
    assert.equal(await store.seal(token, database, now), true);
    assert.equal(other.consume(), null);
  }

  // Review I7: seals stand in for the full check only for a while.
  assert.equal(await store.seal(token, database, now - FULL_CHECK_INTERVAL_MS - 1), true);
  assert.equal(store.consume(now), null, "a full check is due");
  assert.equal(await store.seal(token, database, now + 60_000), true);
  assert.equal(store.consume(now), null, "a clock that went back");

  const locked = new StorageSealStore(mkdtempSync(join(tmpdir(), "elsecrew-seal-")), "/data", "1.0.0", protection(false));
  assert.equal(await locked.seal(token, database, Date.now()), false);
  assert.equal(locked.consume(), null);
});
