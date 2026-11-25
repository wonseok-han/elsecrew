import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { DEV_IDENTITY, SIDECAR_IDENTIFIER, signDevSidecar } from "../scripts/m5-dev-sign-sidecar.mjs";

type Reply = { status: number | null; stdout?: string; stderr?: string };

function harness(replies: Record<string, Reply>) {
  const calls: string[] = [];
  const warnings: string[] = [];
  const run = (command: string, args: string[]) => {
    const key = `${command.split("/").pop()} ${args.join(" ")}`;
    calls.push(key);
    const match = Object.entries(replies).find(([prefix]) => key.startsWith(prefix));
    return match ? { stdout: "", stderr: "", ...match[1] } : { status: 0, stdout: "", stderr: "" };
  };
  return { calls, warnings, run, warn: (m: string) => warnings.push(m), log: () => {} };
}

const identity = { status: 0, stdout: `  1) ABC "${DEV_IDENTITY}"\n     1 valid identities found\n` };

test("without the identity the dev sidecar only warns and is left alone", () => {
  const h = harness({ "security find-identity": { status: 0, stdout: "     0 valid identities found\n" } });
  assert.equal(signDevSidecar({ platform: "darwin", binary: "/b", ...h }), "no-identity");
  assert.equal(h.warnings.length, 1);
  assert.ok(!h.calls.some(call => call.startsWith("codesign")));
});

test("other platforms are skipped without touching tools", () => {
  const h = harness({});
  assert.equal(signDevSidecar({ platform: "win32", binary: "/b", ...h }), "skipped");
  assert.deepEqual(h.calls, []);
});

test("an ad-hoc sidecar is signed with the fixed identifier", () => {
  const h = harness({ "security find-identity": identity, "codesign -d -r-": { status: 0, stdout: '# designated => cdhash H"00"' } });
  assert.equal(signDevSidecar({ platform: "darwin", binary: "/b", ...h }), "signed");
  assert.ok(h.calls.includes(`codesign --force --sign ${DEV_IDENTITY} --identifier ${SIDECAR_IDENTIFIER} /b`));
});

test("an already signed sidecar is not signed again", () => {
  const h = harness({
    "security find-identity": identity,
    "codesign -d -r-": { status: 0, stdout: `designated => identifier "${SIDECAR_IDENTIFIER}" and certificate leaf = H"18"` },
    "codesign -dvv": { status: 0, stderr: `Authority=${DEV_IDENTITY}\n` },
  });
  assert.equal(signDevSidecar({ platform: "darwin", binary: "/b", ...h }), "unchanged");
  assert.ok(!h.calls.some(call => call.startsWith("codesign --force")));
});

test("the same identifier signed by another certificate is signed again", () => {
  const h = harness({
    "security find-identity": identity,
    "codesign -d -r-": { status: 0, stdout: `designated => identifier "${SIDECAR_IDENTIFIER}" and certificate leaf = H"99"` },
    "codesign -dvv": { status: 0, stderr: "Authority=Someone Else\n" },
  });
  assert.equal(signDevSidecar({ platform: "darwin", binary: "/b", ...h }), "signed");
});

test("a signing failure warns and lets the development app start", () => {
  const h = harness({ "security find-identity": identity, "codesign --force": { status: 1 } });
  assert.equal(signDevSidecar({ platform: "darwin", binary: "/b", ...h }), "failed");
  assert.equal(h.warnings.length, 1);
});

test("m5:dev signs right after building the sidecar; packaging does not", () => {
  const scripts = JSON.parse(readFileSync(new URL("../package.json", import.meta.url), "utf8")).scripts;
  assert.match(scripts["m5:dev"], /^cargo build -p elsecrew-sidecar -j 4 && node scripts\/m5-dev-sign-sidecar\.mjs && electron-forge start$/);
  assert.ok(!scripts["m5:package"].includes("m5-dev-sign-sidecar"));
});
