import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import test from "node:test";

import { M5_BRIDGE_V1_METHODS } from "../src/m5/contracts/bridge-v1.ts";
import {
  APP_SCHEMA_VERSION,
  decodeJsonl,
  encodeJsonl,
  EVENTS_V1,
  MAX_CONTROL_LINE_BYTES,
  MAX_SEQUENCE,
  MAX_TEXT_CHUNK_BYTES,
  METHODS_V1,
  parsePrepareQuitResult,
  parseReadyPayload,
  PROTOCOL_VERSION,
  ProtocolSessionV1,
  SIDECAR_SCHEMA_VERSION,
  type EventV1,
  type ResponseV1,
} from "../src/m5/contracts/protocol-v1.ts";

interface ValidCase {
  file: string;
  observeSession?: boolean;
  readyEnvelope?: number;
  prepareQuitEnvelope?: number;
}

interface InvalidCase {
  file: string;
  validator: "transcript" | "prepareQuit";
  envelope?: number;
}

interface GeneratedCase {
  name: string;
  sizeKind: "controlLine" | "textChunk";
  targetBytes: number;
  repeatPath: string[];
  expectedValid: boolean;
  template: Record<string, unknown>;
}

interface FixtureManifest {
  schemaVersion: 1;
  contract: {
    protocolVersion: number;
    sidecarSchemaVersion: number;
    appSchemaVersion: number;
    maxControlLineBytes: number;
    maxTextChunkBytes: number;
    maxSequence: number;
    methods: string[];
    events: string[];
    bridgeMethods: string[];
  };
  valid: ValidCase[];
  invalid: InvalidCase[];
  generated: GeneratedCase[];
}

const fixtureRoot = resolve("tests/fixtures/m5/protocol-v1");
const manifest = JSON.parse(
  readFileSync(resolve(fixtureRoot, "manifest.json"), "utf8"),
) as FixtureManifest;

test("TypeScript가 canonical protocol v1 transcript를 decode하고 동일 byte로 encode한다", () => {
  assert.equal(manifest.schemaVersion, 1);
  assert.deepEqual(
    {
      protocolVersion: PROTOCOL_VERSION,
      sidecarSchemaVersion: SIDECAR_SCHEMA_VERSION,
      appSchemaVersion: APP_SCHEMA_VERSION,
      maxControlLineBytes: MAX_CONTROL_LINE_BYTES,
      maxTextChunkBytes: MAX_TEXT_CHUNK_BYTES,
      maxSequence: MAX_SEQUENCE,
      methods: [...METHODS_V1],
      events: [...EVENTS_V1],
      bridgeMethods: [...M5_BRIDGE_V1_METHODS],
    },
    manifest.contract,
  );
  for (const fixture of manifest.valid) {
    const source = readFileSync(resolve(fixtureRoot, fixture.file), "utf8");
    const lines = jsonlLines(source);
    const session = new ProtocolSessionV1();
    const envelopes = lines.map((line) => {
      const envelope = decodeJsonl(line);
      if (fixture.observeSession !== false) session.observe(envelope);
      assert.equal(encodeJsonl(envelope), line, fixture.file);
      return envelope;
    });

    if (fixture.readyEnvelope !== undefined) {
      const envelope = envelopes[fixture.readyEnvelope];
      assert.equal(envelope.kind, "event");
      parseReadyPayload(envelope as EventV1);
    }
    if (fixture.prepareQuitEnvelope !== undefined) {
      const envelope = envelopes[fixture.prepareQuitEnvelope];
      assert.equal(envelope.kind, "response");
      assert.equal(parsePrepareQuitResult(envelope as ResponseV1).safeToQuit, true);
    }
  }
});

test("TypeScript가 모든 invalid golden fixture를 fail-closed로 거부한다", () => {
  for (const fixture of manifest.invalid) {
    assert.throws(() => validateInvalidFixture(fixture), fixture.file);
  }
});

test("TypeScript가 fixture에 고정한 control line과 text chunk 경계를 지킨다", () => {
  for (const fixture of manifest.generated) {
    const line = materializeBoundary(fixture);
    if (fixture.expectedValid) {
      decodeJsonl(line);
    } else {
      assert.throws(() => decodeJsonl(line), fixture.name);
    }
  }
});

test("공개 bridge v1은 버전된 최소 메서드만 선언하고 범용 IPC를 노출하지 않는다", () => {
  assert.deepEqual([...M5_BRIDGE_V1_METHODS], manifest.contract.bridgeMethods);
  const source = readFileSync(resolve("src/m5/contracts/bridge-v1.ts"), "utf8");
  assert.doesNotMatch(source, /ipcRenderer|\binvoke\s*\(/);
});

function validateInvalidFixture(fixture: InvalidCase): void {
  const source = readFileSync(resolve(fixtureRoot, fixture.file), "utf8");
  const session = new ProtocolSessionV1();
  const envelopes = jsonlLines(source).map((line) => {
    const envelope = decodeJsonl(line);
    session.observe(envelope);
    return envelope;
  });
  if (fixture.validator === "prepareQuit") {
    const envelope = envelopes[requiredIndex(fixture.envelope)];
    if (envelope.kind !== "response") throw new Error("fixture is not a response");
    parsePrepareQuitResult(envelope);
  }
}

function jsonlLines(source: string): string[] {
  if (!source.endsWith("\n")) throw new Error("fixture must end with a JSONL delimiter");
  return source
    .slice(0, -1)
    .split("\n")
    .map((line) => `${line}\n`);
}

function materializeBoundary(fixture: GeneratedCase): string {
  const value = structuredClone(fixture.template);
  if (fixture.sizeKind === "textChunk") {
    setPath(value, fixture.repeatPath, "a".repeat(fixture.targetBytes));
  } else {
    setPath(value, fixture.repeatPath, "");
    const empty = JSON.stringify(value);
    const paddingBytes = fixture.targetBytes - Buffer.byteLength(empty);
    if (paddingBytes < 0) throw new Error("control fixture target is smaller than its template");
    setPath(value, fixture.repeatPath, "a".repeat(paddingBytes));
  }
  const line = `${JSON.stringify(value)}\n`;
  if (fixture.sizeKind === "controlLine") {
    assert.equal(Buffer.byteLength(line.slice(0, -1)), fixture.targetBytes, fixture.name);
  }
  return line;
}

function setPath(root: Record<string, unknown>, path: string[], value: string): void {
  let target = root;
  for (const segment of path.slice(0, -1)) {
    const next = target[segment];
    if (typeof next !== "object" || next === null || Array.isArray(next)) {
      throw new Error(`invalid repeatPath at ${segment}`);
    }
    target = next as Record<string, unknown>;
  }
  target[path.at(-1) ?? ""] = value;
}

function requiredIndex(value: number | undefined): number {
  if (value === undefined) throw new Error("fixture envelope index is required");
  return value;
}
