import test from "node:test";
import assert from "node:assert/strict";
import { validateInstallRequest, validateInstallStatus } from "../src/m5/contracts/provider-install-v1.ts";
const id = "11111111-1111-4111-8111-111111111111";
const inactive = { state: "unsupported", operationId: null, provider: null, plan: null, error: null, cancelRequested: false };
const plan = { planId: id, provider: "codex", action: "installOrUpdate", expiresInSeconds: 180,
  writeDirectories: ["/Users/example/.local/bin", "/Users/example/.codex/packages/standalone"],
  replacesExistingCommands: true, shellSettingsWritable: false, startsLogin: false };
const consent = { ...inactive, state: "awaitingConsent", provider: "codex", plan };
test("installation boundary accepts exact requests and consent state", () => {
  assert.deepEqual(validateInstallRequest({ action: "start", provider: "codex", planId: id }), { action: "start", provider: "codex", planId: id });
  assert.deepEqual(validateInstallStatus(consent), consent);
  assert.deepEqual(validateInstallStatus(inactive), inactive);
});
test("installation boundary rejects extra authority and mismatched destinations", () => {
  for (const request of [{ action: "status", command: "sh" }, { action: "prepare", provider: "codex", path: "/tmp" },
    { action: "start", provider: "codex", planId: "unknown" }, { action: "cancel", operationId: "00000000-0000-0000-0000-000000000000" }]) {
    assert.throws(() => validateInstallRequest(request));
  }
  for (const invalid of [{ ...plan, startsLogin: true }, { ...plan, shellSettingsWritable: true },
    { ...plan, writeDirectories: ["/Users/example/.local/bin", "/Users/other/.codex/packages/standalone"] },
    { ...plan, writeDirectories: ["/Users/example/../other/.local/bin", "/Users/example/../other/.codex/packages/standalone"] },
    { ...plan, script: "arbitrary" }]) assert.throws(() => validateInstallStatus({ ...consent, plan: invalid }));
});
test("installation state cannot claim an unbound operation or implicit login", () => {
  for (const invalid of [{ ...inactive, state: "installed" }, { ...consent, provider: "claude" },
    { ...inactive, cancelRequested: true }, { ...consent, state: "installing" },
    { ...inactive, authUrl: "https://example.com" }]) assert.throws(() => validateInstallStatus(invalid));
});
