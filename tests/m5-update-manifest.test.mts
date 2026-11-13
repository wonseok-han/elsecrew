import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { generateKeyPairSync, sign } from "node:crypto";
import { mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { compareVersions, platformKey, selectUpdate, UPDATE_PUBLIC_KEY_PEM, verifyFeed } from "../electron/update-manifest.ts";

const keys = () => {
  const { publicKey, privateKey } = generateKeyPairSync("ed25519");
  return { publicPem: publicKey.export({ type: "spki", format: "pem" }).toString(), privatePem: privateKey.export({ type: "pkcs8", format: "pem" }).toString(), privateKey };
};
const asset = { name: "Elsecrew-darwin-arm64-0.2.0.zip", sha256: "a".repeat(64), size: 1000 };
function feed(manifest: unknown, privateKey: ReturnType<typeof keys>["privateKey"]): string {
  const bytes = Buffer.from(JSON.stringify(manifest), "utf8");
  return JSON.stringify({ schemaVersion: 1, manifest: bytes.toString("base64"), signature: sign(null, bytes, privateKey).toString("base64") });
}

test("the release script's feed verifies in the app and selects this machine's newer package", () => {
  const { publicPem, privatePem } = keys();
  const dir = mkdtempSync(join(tmpdir(), "elsecrew-feed-"));
  for (const platform of ["darwin-arm64", "darwin-x64", "win32-x64"]) writeFileSync(join(dir, `Elsecrew-${platform}-0.2.0.zip`), `package ${platform}`);
  execFileSync(process.execPath, ["scripts/m5-release-manifest.mjs", "--version", "0.2.0", "--out", join(dir, "latest.json"),
    ...["darwin-arm64", "darwin-x64", "win32-x64"].map(platform => join(dir, `Elsecrew-${platform}-0.2.0.zip`))],
  { env: { ...process.env, ELSECREW_UPDATE_PRIVATE_KEY: privatePem } });
  const manifest = verifyFeed(readFileSync(join(dir, "latest.json"), "utf8"), publicPem);
  assert.equal(manifest.version, "0.2.0");
  assert.equal(manifest.assets["darwin-arm64"]!.size, "package darwin-arm64".length);
  const update = selectUpdate(manifest, "0.1.2", "darwin-arm64");
  assert.equal(update?.url, "https://github.com/wonseok-han/elsecrew/releases/download/v0.2.0/Elsecrew-darwin-arm64-0.2.0.zip");
  assert.equal(selectUpdate(manifest, "0.2.0", "darwin-arm64"), null, "same version is current");
  assert.equal(selectUpdate(manifest, "0.3.0", "darwin-arm64"), null, "never downgrades");
});

test("tampering, another key, unknown fields and unsafe names are rejected", () => {
  const signer = keys();
  const other = keys();
  const manifest = { schemaVersion: 1, product: "Elsecrew", version: "0.2.0", assets: { "darwin-arm64": asset } };
  const good = feed(manifest, signer.privateKey);
  assert.doesNotThrow(() => verifyFeed(good, signer.publicPem));
  assert.throws(() => verifyFeed(good, other.publicPem), /signature/);
  const envelope = JSON.parse(good);
  const changed = Buffer.from(JSON.stringify({ ...manifest, version: "9.9.9" })).toString("base64");
  assert.throws(() => verifyFeed(JSON.stringify({ ...envelope, manifest: changed }), signer.publicPem), /signature/);
  assert.throws(() => verifyFeed(JSON.stringify({ ...envelope, extra: 1 }), signer.publicPem), /fields/);
  const bad = (patch: Record<string, unknown>) => feed({ ...manifest, assets: { "darwin-arm64": { ...asset, ...patch } } }, signer.privateKey);
  assert.throws(() => verifyFeed(bad({ name: "../evil.zip" }), signer.publicPem), /asset name/);
  assert.throws(() => verifyFeed(bad({ sha256: "x" }), signer.publicPem), /digest/);
  assert.throws(() => verifyFeed(bad({ size: 0 }), signer.publicPem), /size/);
  assert.throws(() => verifyFeed(feed({ ...manifest, assets: { "linux-x64": asset } }, signer.privateKey), signer.publicPem), /platform/);
  assert.throws(() => verifyFeed(feed({ ...manifest, version: "1.0" }, signer.privateKey), signer.publicPem), /header/);
  assert.throws(() => verifyFeed("not json", signer.publicPem), /JSON/);
  // The shipped public key never accepts a test signer.
  assert.throws(() => verifyFeed(good, UPDATE_PUBLIC_KEY_PEM), /signature/);
});

test("versions compare numerically and only supported platforms update", () => {
  assert.equal(compareVersions("0.10.0", "0.9.9"), 1);
  assert.equal(compareVersions("1.0.0", "1.0.0"), 0);
  assert.equal(compareVersions("0.1.2", "0.1.10"), -1);
  assert.equal(platformKey("darwin", "arm64"), "darwin-arm64");
  assert.equal(platformKey("win32", "x64"), "win32-x64");
  assert.equal(platformKey("linux", "x64"), null);
  assert.equal(platformKey("win32", "arm64"), null);
});
