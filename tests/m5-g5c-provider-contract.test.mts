import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import test from "node:test";

import {
  AGENT_PUBLIC_EVENT_KINDS_V1,
  G5A_CAPABILITIES_V1,
  G5C_CLAUDE_RUNTIME_VERIFICATION_V1,
  G5C_CODEX_RUNTIME_VERIFICATION_V1,
  G5C_PROVIDER_EVENT_CLASSIFICATIONS_V1,
  canonicalProviderExecutableClosureV1,
  canonicalProviderManifestV1,
  computeProviderExecutableClosureDigestV1,
  computeProviderManifestDigestV1,
  validateAgentRuntimeAttestationAgainstAcceptedV1,
  validateInternalProviderEventSequenceV1,
  validateProviderManifestV1,
  validateSanitizedDiscoveryObservationV1,
} from "../src/m5/contracts/agent-adapter-v1.ts";
import { M5_BRIDGE_V4_METHODS } from "../src/m5/contracts/bridge-v4.ts";
import { M5_BRIDGE_V5_METHODS } from "../src/m5/contracts/bridge-v5.ts";
import {
  AGENT_PUBLIC_EVENTS_PER_RUN_MAX_V1,
  AGENT_PUBLIC_TEXT_BYTES_PER_RUN_MAX_V1,
  EVENTS_V1,
} from "../src/m5/contracts/protocol-v1.ts";
import { G5_CAPABILITIES_V1, G5_PUBLIC_MESSAGE_MAX_BYTES_V1 } from "../src/m5/contracts/run-recovery-v1.ts";

interface MutationChange { pointer: string; value: unknown }
interface MutationCase {
  name: string;
  target: "claudeManifest" | "claudeObservation" | "claudeAttestation" | "acceptedRun" | "resolvedProfile" | "claudeEvents";
  changes: MutationChange[];
}
interface GeneratedEventCase {
  name: string;
  valid: boolean;
  generator: "uniformPublicEvents" | "publicTextByteLengths";
  publicEventCount?: number;
  publicTextBytesEach?: number;
  publicTextByteLengths?: number[];
  expectedPublicEventCount: number;
  expectedPublicTextBytes: number;
}
interface G5CCases {
  claudeManifest: unknown;
  claudeObservation: unknown;
  acceptedRun: unknown;
  resolvedProfile: unknown;
  claudeAttestation: unknown;
  claudeEvents: unknown;
  codexManifest: unknown;
  codexArgvOnlyObservation: unknown;
  codexUnknownCatalogObservation: unknown;
  invalidMutations: MutationCase[];
  generatedEventCases: GeneratedEventCase[];
}

const root = resolve("tests/fixtures/m5/agent-adapter-v1");
const adapterManifest = JSON.parse(readFileSync(resolve(root, "manifest.json"), "utf8"));
const cases = JSON.parse(readFileSync(resolve(root, adapterManifest.g5cCasesFile), "utf8")) as G5CCases;

test("G5C canonical manifests bind the complete executable closure, exact tuples, parsers, and no-fallback launch policy", () => {
  const claude = validateProviderManifestV1(cases.claudeManifest);
  const codex = validateProviderManifestV1(cases.codexManifest);

  assert.equal(computeProviderExecutableClosureDigestV1(claude.executableClosure), claude.executableClosure.closureDigest);
  assert.equal(computeProviderManifestDigestV1(claude), claude.manifestDigest);
  assert.equal(computeProviderExecutableClosureDigestV1(codex.executableClosure), codex.executableClosure.closureDigest);
  assert.equal(computeProviderManifestDigestV1(codex), codex.manifestDigest);
  assert.equal(JSON.parse(canonicalProviderExecutableClosureV1(claude.executableClosure)).closureDigest, undefined);
  assert.equal(JSON.parse(canonicalProviderManifestV1(claude)).manifestDigest, undefined);
  assert.deepEqual(claude.launchPolicy.requiredRuntimeVerification, [...G5C_CLAUDE_RUNTIME_VERIFICATION_V1]);
  assert.deepEqual(codex.launchPolicy.requiredRuntimeVerification, [...G5C_CODEX_RUNTIME_VERIFICATION_V1]);
  assert.equal(claude.launchPolicy.fallbackPolicy, "forbidden");
  assert.ok(claude.executableClosure.requiredHelpers.length > 0);
});

test("G5C sanitized discovery requires all Claude runtime evidence and keeps Codex argvOnly or unknown catalogs blocked", () => {
  const claude = validateSanitizedDiscoveryObservationV1(cases.claudeObservation, cases.claudeManifest);
  const argvOnly = validateSanitizedDiscoveryObservationV1(cases.codexArgvOnlyObservation, cases.codexManifest);
  const unknown = validateSanitizedDiscoveryObservationV1(cases.codexUnknownCatalogObservation, cases.codexManifest);

  assert.equal(claude.canStart, true);
  assert.deepEqual(claude.observedRuntimeVerification, ["version", "model", "cwd", "permission", "tools", "mcp", "plugins", "config"]);
  assert.deepEqual([argvOnly.canStart, argvOnly.blockedReason], [false, "modelCatalogUnverified"]);
  assert.deepEqual([unknown.canStart, unknown.blockedReason], [false, "modelCatalogUnverified"]);
  for (const forbidden of ["rawStdout", "rawStderr", "token", "prompt", "accountEmail", "organization", "rawAuthHash"]) {
    assert.equal(Object.hasOwn(claude, forbidden), false);
  }
});

test("G5C attestation binds launch identity and internal events expose only handshake, existing public kinds, private classes, and terminal state", () => {
  const attestation = validateAgentRuntimeAttestationAgainstAcceptedV1(
    cases.claudeAttestation,
    cases.claudeManifest,
    cases.claudeObservation,
    cases.acceptedRun,
    cases.resolvedProfile,
  );
  const events = validateInternalProviderEventSequenceV1(cases.claudeEvents, cases.claudeManifest);

  assert.equal(attestation.runId, "agent_run_g5c_claude_01");
  assert.equal(attestation.workspaceIdentity, "workspace_01");
  assert.equal(Object.hasOwn(attestation, "sessionDigest"), false);
  assert.match(attestation.managedCwdIdentity, /^sha256:[0-9a-f]{64}$/);
  assert.deepEqual([...new Set(events.map((event) => event.classification))], G5C_PROVIDER_EVENT_CLASSIFICATIONS_V1.slice(0, 4));
  assert.ok(events.filter((event) => event.classification === "Public")
    .every((event) => AGENT_PUBLIC_EVENT_KINDS_V1.includes(event.kind)));
  assert.match(JSON.stringify(events), /프로젝트 변경 범위를 확인하고 있습니다/);
});

test("G5C exact invalid mutations reject raw/private/auth identity leaks, incomplete closure, mismatched digests, unsupported tuples, and event ordering", () => {
  for (const mutation of cases.invalidMutations) {
    const values = {
      claudeManifest: structuredClone(cases.claudeManifest),
      claudeObservation: structuredClone(cases.claudeObservation),
      claudeAttestation: structuredClone(cases.claudeAttestation),
      acceptedRun: structuredClone(cases.acceptedRun),
      resolvedProfile: structuredClone(cases.resolvedProfile),
      claudeEvents: structuredClone(cases.claudeEvents),
    };
    const value = values[mutation.target];
    for (const change of mutation.changes) setJsonPointer(value, change.pointer, materializeValue(change.value));
    const validate = () => {
      if (mutation.target === "claudeManifest") validateProviderManifestV1(value);
      else if (mutation.target === "claudeObservation") validateSanitizedDiscoveryObservationV1(value, cases.claudeManifest);
      else if (mutation.target === "claudeEvents") validateInternalProviderEventSequenceV1(value, cases.claudeManifest);
      else validateAgentRuntimeAttestationAgainstAcceptedV1(
        values.claudeAttestation,
        cases.claudeManifest,
        cases.claudeObservation,
        values.acceptedRun,
        values.resolvedProfile,
      );
    };
    assert.throws(validate, undefined, mutation.name);
  }
});

test("G5C internal provider events enforce the existing G5B 256-event and 1 MiB aggregate public-text budgets", () => {
  for (const entry of cases.generatedEventCases) {
    const events = generateProviderEvents(entry);
    const validate = () => validateInternalProviderEventSequenceV1(events, cases.claudeManifest);
    if (entry.valid) assert.doesNotThrow(validate, entry.name);
    else assert.throws(validate, undefined, entry.name);
    const publicEvents = events.filter((event) => event.classification === "Public");
    assert.equal(publicEvents.length, entry.expectedPublicEventCount, entry.name);
    assert.equal(
      publicEvents.reduce((total, event) => total + new TextEncoder().encode(event.text as string).length, 0),
      entry.expectedPublicTextBytes,
      entry.name,
    );
    assert.ok(
      publicEvents.every((event) => new TextEncoder().encode(event.text as string).length <= G5_PUBLIC_MESSAGE_MAX_BYTES_V1),
      entry.name,
    );
    if (entry.expectedPublicTextBytes >= AGENT_PUBLIC_TEXT_BYTES_PER_RUN_MAX_V1) {
      assert.ok(publicEvents.length < AGENT_PUBLIC_EVENTS_PER_RUN_MAX_V1, entry.name);
    }
  }
});

test("G5C remains internal and preserves the exact G5/G5A/G5B public capability, event, and V4/V5 bridge sets", () => {
  assert.deepEqual(adapterManifest.contract.capabilities, [...G5A_CAPABILITIES_V1]);
  assert.deepEqual(G5A_CAPABILITIES_V1.slice(0, G5_CAPABILITIES_V1.length), G5_CAPABILITIES_V1);
  assert.deepEqual(M5_BRIDGE_V5_METHODS.slice(0, M5_BRIDGE_V4_METHODS.length), M5_BRIDGE_V4_METHODS);
  assert.deepEqual(M5_BRIDGE_V5_METHODS.slice(M5_BRIDGE_V4_METHODS.length), ["onAgentRunStateChanged"]);
  assert.deepEqual(EVENTS_V1.slice(-2), ["agent.publicEvent", "agent.runStateChanged"]);
  assert.equal(EVENTS_V1.some((event) => event.includes("provider") || event.includes("attestation")), false);
});

function materializeValue(value: unknown): unknown {
  return value === "__G5C_OVERSIZE_PUBLIC_TEXT__"
    ? "a".repeat(G5_PUBLIC_MESSAGE_MAX_BYTES_V1 + 1)
    : structuredClone(value);
}

function setJsonPointer(rootValue: unknown, pointer: string, value: unknown): void {
  const segments = pointer.slice(1).split("/").map((segment) => segment.replaceAll("~1", "/").replaceAll("~0", "~"));
  let cursor = rootValue as Record<string, unknown> | unknown[];
  for (const segment of segments.slice(0, -1)) {
    cursor = (cursor as Record<string, Record<string, unknown> | unknown[]>)[segment];
  }
  const last = segments.at(-1)!;
  (cursor as Record<string, unknown>)[last] = value;
}

function generateProviderEvents(entry: GeneratedEventCase): Record<string, unknown>[] {
  const lengths = entry.generator === "uniformPublicEvents"
    ? Array.from({ length: entry.publicEventCount! }, () => entry.publicTextBytesEach!)
    : entry.publicTextByteLengths!;
  const events: Record<string, unknown>[] = [{
    schemaVersion: 1,
    sequence: 1,
    classification: "Handshake",
    verifiedRuntimeEvidence: [...G5C_CLAUDE_RUNTIME_VERIFICATION_V1],
  }];
  for (const [index, length] of lengths.entries()) {
    events.push({
      schemaVersion: 1,
      sequence: index + 2,
      classification: "Public",
      kind: "status",
      text: "a".repeat(length),
    });
  }
  events.push({
    schemaVersion: 1,
    sequence: lengths.length + 2,
    classification: "Terminal",
    outcome: "succeeded",
  });
  return events;
}
