#!/usr/bin/env node
// Writes the signed update feed `latest.json` for a release.
//
//   ELSECREW_UPDATE_PRIVATE_KEY="<PKCS#8 PEM>" \
//   node scripts/m5-release-manifest.mjs --version 0.1.3 --out dist/latest.json dist/*.zip
//
// Each ZIP name must contain its platform (darwin-arm64, darwin-x64 or
// win32-x64). The manifest bytes are signed with Ed25519 exactly as written;
// the app verifies them against the public key in electron/update-manifest.ts.
import { createHash, createPrivateKey, sign } from "node:crypto";
import { createReadStream, statSync, writeFileSync } from "node:fs";
import { basename } from "node:path";

const args = process.argv.slice(2);
const option = name => {
  const index = args.indexOf(name);
  if (index < 0 || !args[index + 1]) throw new Error(`missing ${name}`);
  const value = args[index + 1];
  args.splice(index, 2);
  return value;
};
const version = option("--version");
const out = option("--out");
const zips = args;
if (!/^(0|[1-9]\d{0,5})\.(0|[1-9]\d{0,5})\.(0|[1-9]\d{0,5})$/.test(version)) throw new Error(`invalid version ${version}`);
const pem = process.env.ELSECREW_UPDATE_PRIVATE_KEY;
if (!pem) throw new Error("ELSECREW_UPDATE_PRIVATE_KEY is not set");
const key = createPrivateKey(pem);
if (key.asymmetricKeyType !== "ed25519") throw new Error("the update key must be Ed25519");

const sha256 = file => new Promise((resolveHash, reject) => {
  const hash = createHash("sha256");
  createReadStream(file).on("data", chunk => hash.update(chunk)).on("error", reject).on("end", () => resolveHash(hash.digest("hex")));
});

const assets = {};
for (const file of zips) {
  const name = basename(file);
  const platform = ["darwin-arm64", "darwin-x64", "win32-x64"].find(candidate => name.includes(candidate));
  if (!platform) throw new Error(`cannot tell the platform of ${name}`);
  if (assets[platform]) throw new Error(`two packages for ${platform}`);
  if (!name.endsWith(`${version}.zip`)) throw new Error(`${name} does not match version ${version}`);
  assets[platform] = { name, sha256: await sha256(file), size: statSync(file).size };
}
if (!Object.keys(assets).length) throw new Error("no packages given");

const manifest = Buffer.from(JSON.stringify({ schemaVersion: 1, product: "Elsecrew", version, assets }), "utf8");
const feed = { schemaVersion: 1, manifest: manifest.toString("base64"), signature: sign(null, manifest, key).toString("base64") };
writeFileSync(out, `${JSON.stringify(feed)}\n`);
console.log(`signed ${Object.keys(assets).join(", ")} for ${version} → ${out}`);
