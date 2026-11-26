// Self-hosted update channel without paid code signing: the app stays
// unsigned, and every release publishes `latest.json`, a manifest signed with
// the project's own Ed25519 key. The app trusts only manifests that verify
// against the public key below, builds download URLs itself, and accepts a
// package only when its size and SHA-256 match the signed manifest.
import { createPublicKey, verify } from "node:crypto";

export const UPDATE_PUBLIC_KEY_PEM = `-----BEGIN PUBLIC KEY-----
MCowBQYDK2VwAyEAgmghhDzB8CEj3Gnve+16Et7RdOV/yWNypCRx/fpfslo=
-----END PUBLIC KEY-----
`;
export const UPDATE_FEED_URL = "https://github.com/wonseok-han/elsecrew/releases/latest/download/latest.json";
const DOWNLOAD_BASE = "https://github.com/wonseok-han/elsecrew/releases/download/";
export const MAX_FEED_BYTES = 64 * 1024;
export const MAX_PACKAGE_BYTES = 1024 * 1024 * 1024;

export type UpdatePlatform = "darwin-arm64" | "darwin-x64" | "win32-x64";
export interface UpdateAsset { name: string; sha256: string; size: number }
export interface UpdateManifest { schemaVersion: 1; product: "Elsecrew"; version: string; assets: Partial<Record<UpdatePlatform, UpdateAsset>> }
export interface VerifiedUpdate { version: string; asset: UpdateAsset; url: string }

const VERSION = /^(0|[1-9]\d{0,5})\.(0|[1-9]\d{0,5})\.(0|[1-9]\d{0,5})$/;
const ASSET_NAME = /^[A-Za-z0-9][A-Za-z0-9._-]{0,127}\.zip$/;
const PLATFORMS: readonly UpdatePlatform[] = ["darwin-arm64", "darwin-x64", "win32-x64"];

function fail(reason: string): never { throw new Error(`update manifest rejected: ${reason}`); }

function exactKeys(value: Record<string, unknown>, keys: readonly string[], name: string): void {
  const actual = Object.keys(value).sort().join(",");
  if (actual !== [...keys].sort().join(",")) fail(`${name} fields`);
}

function object(value: unknown, name: string): Record<string, unknown> {
  if (!value || typeof value !== "object" || Array.isArray(value)) fail(`${name} is not an object`);
  return value as Record<string, unknown>;
}

/** 1 when `a` is newer than `b`, -1 when older, 0 when equal. */
export function compareVersions(a: string, b: string): number {
  const left = a.match(VERSION), right = b.match(VERSION);
  if (!left || !right) fail("version format");
  for (let index = 1; index <= 3; index++) {
    const difference = Number(left[index]) - Number(right[index]);
    if (difference) return Math.sign(difference);
  }
  return 0;
}

export function platformKey(platform: string, arch: string): UpdatePlatform | null {
  const key = `${platform}-${arch}`;
  return (PLATFORMS as readonly string[]).includes(key) ? key as UpdatePlatform : null;
}

/**
 * Verifies the signed envelope `{schemaVersion, manifest, signature}` where
 * `manifest` is base64 of the exact UTF-8 JSON bytes that were signed.
 */
export function verifyFeed(raw: string, publicKeyPem: string = UPDATE_PUBLIC_KEY_PEM): UpdateManifest {
  if (Buffer.byteLength(raw) > MAX_FEED_BYTES) fail("feed too large");
  let envelope: Record<string, unknown>;
  try { envelope = object(JSON.parse(raw), "feed"); } catch (error) { if (error instanceof Error && error.message.startsWith("update manifest rejected")) throw error; fail("feed is not JSON"); }
  exactKeys(envelope, ["schemaVersion", "manifest", "signature"], "feed");
  if (envelope.schemaVersion !== 1 || typeof envelope.manifest !== "string" || typeof envelope.signature !== "string") fail("feed envelope");
  const bytes = Buffer.from(envelope.manifest, "base64");
  const signature = Buffer.from(envelope.signature, "base64");
  if (bytes.toString("base64") !== envelope.manifest || signature.length !== 64) fail("encoding");
  if (!verify(null, bytes, createPublicKey(publicKeyPem), signature)) fail("signature");
  let manifest: Record<string, unknown>;
  try { manifest = object(JSON.parse(bytes.toString("utf8")), "manifest"); } catch { fail("manifest is not JSON"); }
  exactKeys(manifest, ["schemaVersion", "product", "version", "assets"], "manifest");
  if (manifest.schemaVersion !== 1 || manifest.product !== "Elsecrew" || typeof manifest.version !== "string" || !VERSION.test(manifest.version)) fail("manifest header");
  const assets = object(manifest.assets, "assets");
  for (const [key, value] of Object.entries(assets)) {
    if (!(PLATFORMS as readonly string[]).includes(key)) fail(`unknown platform ${key}`);
    const asset = object(value, "asset");
    exactKeys(asset, ["name", "sha256", "size"], "asset");
    if (typeof asset.name !== "string" || !ASSET_NAME.test(asset.name)) fail("asset name");
    if (typeof asset.sha256 !== "string" || !/^[0-9a-f]{64}$/.test(asset.sha256)) fail("asset digest");
    if (!Number.isSafeInteger(asset.size) || (asset.size as number) <= 0 || (asset.size as number) > MAX_PACKAGE_BYTES) fail("asset size");
  }
  return manifest as unknown as UpdateManifest;
}

/** The newer version's asset for this machine, or null when current. */
export function selectUpdate(manifest: UpdateManifest, currentVersion: string, platform: UpdatePlatform): VerifiedUpdate | null {
  if (compareVersions(manifest.version, currentVersion) <= 0) return null;
  const asset = manifest.assets[platform];
  if (!asset) return null;
  return { version: manifest.version, asset, url: `${DOWNLOAD_BASE}v${manifest.version}/${encodeURIComponent(asset.name)}` };
}
