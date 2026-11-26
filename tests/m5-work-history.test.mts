import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { maskSecrets, SECRET_MASK } from "../src/m5/secret-shape.ts";
import { activityDetail, activityItems, activityLine, BLOCK_MAX, displayLine, durationParts, LINE_MAX, openableFile, parseWorkActivity } from "../src/m5/work-history.ts";

const char = (code: number) => String.fromCharCode(code);
const REPLACED = char(0xfffd);
/** Synthetic values only; each says it is fake. */
const FAKE_GITHUB = `ghp_${"FAKEexample0".repeat(3)}`;
const FAKE_OPENAI = "sk-FAKE-example-not-a-real-key-0000";

test("work history rows show the kind of work and never unknown fields", () => {
  assert.deepEqual(parseWorkActivity('{"action":"read","target":"src/app.ts"}'), { action: "read", target: "src/app.ts" });
  assert.deepEqual(parseWorkActivity('{"action":"command","outcome":"refused"}'), { action: "command", outcome: "refused" });
  assert.deepEqual(parseWorkActivity('{"action":"command","outcome":"stopped","id":"t3","phase":"done"}'), { action: "command", outcome: "stopped", id: "t3", phase: "done" });
  assert.deepEqual(parseWorkActivity('{"action":"other","tool":"WebFetch"}'), { action: "other", tool: "WebFetch" });
  assert.deepEqual(parseWorkActivity('{"action":"delete","target":"old.md","id":"c1:2"}'), { action: "delete", target: "old.md", id: "c1:2" });
  for (const text of [
    "plain model text", "[]", "null",
    '{"action":"move"}',
    '{"action":"read","secret":"x"}',
    '{"action":"command","output":"what a command printed is never a field"}',
    '{"action":"read","target":""}',
    '{"action":"read","outcome":"maybe"}',
    '{"action":"read","phase":"started"}',
    '{"action":"read","id":"toolu_01/../x"}',
    '{"action":"command","exit":"0"}',
    '{"action":"command","ms":-1}',
    '{"action":"edit","added":1.5}',
    `{"action":"read","target":"${"a".repeat(20000)}"}`,
  ]) assert.equal(parseWorkActivity(text), null, text);
  const emoji = "😀".repeat(100);
  assert.deepEqual(parseWorkActivity(JSON.stringify({ action: "read", target: emoji })), { action: "read", target: emoji });
});

test("control, separator and direction marks are neutralised; lines and blocks are capped", () => {
  assert.equal(parseWorkActivity(JSON.stringify({ action: "read", target: `file${char(0x202e)}txt.exe` }))!.target, `file${REPLACED}txt.exe`);
  assert.equal(parseWorkActivity(JSON.stringify({ action: "read", target: "a\nb" }))!.target, `a${REPLACED}b`);
  assert.equal(parseWorkActivity(JSON.stringify({ action: "read", target: `a${char(0x2028)}b${char(0x2029)}c` }))!.target, `a${REPLACED}b${REPLACED}c`);
  assert.equal(parseWorkActivity(JSON.stringify({ action: "command", target: "x", command: `echo ${char(0x1b)}[31mred${char(7)}\nnext\r\nlast\tend\rlone` }))!.command, `echo ${REPLACED}[31mred${REPLACED}\nnext\nlast\tend${REPLACED}lone`);
  const longLine = parseWorkActivity(JSON.stringify({ action: "command", target: "x".repeat(400) }))!.target!;
  assert.equal(Array.from(longLine).length, LINE_MAX + 1);
  assert.ok(longLine.endsWith("…"));
  const longBlock = parseWorkActivity(JSON.stringify({ action: "command", command: "y".repeat(9000) }))!.command!;
  assert.equal(Array.from(longBlock).length, BLOCK_MAX + 1);
});

test("secrets are masked with the same rules as the engine (shared cases, review bypasses included)", () => {
  const cases = JSON.parse(readFileSync(new URL("./fixtures/m5/secret-mask-cases.json", import.meta.url), "utf8")) as { mask: { text: string; masked: string }[]; line: { text: string; shown: string }[] };
  assert.ok(cases.mask.length >= 45 && cases.line.length >= 5);
  // Shapes a secret scanner reads as real are stored with {colon}/{dot}.
  for (const { text: stored, masked } of cases.mask) {
    const text = stored.replaceAll("{colon}", ":").replaceAll("{dot}", ".");
    assert.equal(maskSecrets(text), masked, JSON.stringify(text));
    assert.equal(maskSecrets(masked), masked, `masking twice changes nothing: ${JSON.stringify(text)}`);
    // No fake secret value survives masking (each has FAKE or fake-example
    // in it; user names like "fakeuser" are not secrets and stay).
    for (const word of text.split(/[\s"'=:,;&?@]+/)) {
      if (/FAKE|fake[-.]?example/.test(word) && masked.includes(word)) assert.fail(`${word} kept in ${JSON.stringify(masked)}`);
    }
  }
  for (const { text, shown } of cases.line) assert.equal(displayLine(text), shown, JSON.stringify(text));
  // A row stored by an older build, unmasked, is masked when shown.
  const old = parseWorkActivity(JSON.stringify({ action: "command", target: "curl -H 'Authorization: Bearer fakeexampletoken01' https://api.example.com" }))!;
  assert.equal(old.target, `curl -H 'Authorization: ${SECRET_MASK} ${SECRET_MASK}' https://api.example.com`);
  // Masked before the cut: a key at the line's end leaves nothing (review B2).
  assert.ok(!displayLine(`${"p".repeat(150)} ${FAKE_OPENAI}`).includes("FAKE"));
  assert.ok(!parseWorkActivity(JSON.stringify({ action: "command", target: `${"q".repeat(152)} GITHUB_TOKEN=${FAKE_GITHUB}` }))!.target!.includes("FAKE"));
});

test("a line names what was worked on; a command folds open to its detail, without output", () => {
  const command = parseWorkActivity(JSON.stringify({ action: "command", target: "npm test", command: "npm test", cwd: ".", exit: 1, ms: 1500, outcome: "failed", phase: "done", id: "c1" }))!;
  assert.deepEqual(activityLine(command), { label: "m5.output.activity.command", target: "npm test", file: null });
  assert.deepEqual(activityDetail(command), { command: "npm test", cwd: ".", note: null, exit: 1, ms: 1500, lines: null });
  assert.equal(activityDetail({ action: "command", target: "ls", command: "ls" }), null);
  assert.equal(activityDetail({ action: "read", target: "src/app.ts" }), null);
  const edit = parseWorkActivity(JSON.stringify({ action: "edit", target: "src/app.ts", added: 3, removed: 1, phase: "done", id: "c2:0" }))!;
  assert.equal(activityLine(edit).file, "src/app.ts");
  assert.deepEqual(activityDetail(edit)!.lines, { added: 3, removed: 1 });
  assert.deepEqual(durationParts(0), { key: "m5.output.activity.instant", values: {} });
  assert.deepEqual(durationParts(400), { key: "m5.output.activity.ms", values: { s: 0.4 } });
  assert.deepEqual(durationParts(12_300), { key: "m5.output.activity.seconds", values: { s: 12 } });
  assert.deepEqual(durationParts(125_000), { key: "m5.output.activity.minutes", values: { m: 2, s: 5 } });
});

test("a line opens a file only when the task's change list has that exact path (review I1)", () => {
  const changed = new Set(["src/app.ts", "notes/ready.md"]);
  assert.equal(openableFile(activityLine({ action: "edit", target: "src/app.ts" }).file, changed), "src/app.ts");
  // Not in the list, masked or cut, refused or failed, reading, outside the workspace, or no list yet.
  assert.equal(openableFile(activityLine({ action: "edit", target: "src/other.ts" }).file, changed), null);
  assert.equal(openableFile(activityLine({ action: "edit", target: `keys/sk-${SECRET_MASK}.txt` }).file, changed), null);
  assert.equal(openableFile(activityLine({ action: "edit", target: "src/app.ts", outcome: "refused" }).file, changed), null);
  assert.equal(openableFile(activityLine({ action: "write", target: "src/app.ts", outcome: "failed" }).file, changed), null);
  assert.equal(openableFile(activityLine({ action: "read", target: "src/app.ts" }).file, changed), null);
  assert.equal(openableFile(activityLine({ action: "edit", target: "…/etc/hosts" }).file, changed), null);
  assert.equal(openableFile(activityLine({ action: "edit", target: "src/app.ts" }).file, null), null);
});

test("each closing row is used once, by its own first row; same-file changes read as one line (review I2, M2)", () => {
  const rows = [
    { key: "1", text: JSON.stringify({ action: "command", target: "npm test", command: "npm test", id: "c1" }) },
    { key: "2", text: null as string | null },
    { key: "3", text: JSON.stringify({ action: "write", target: "src/app.ts", id: "c2:0" }) },
    { key: "4", text: JSON.stringify({ action: "command", target: "npm test", command: "npm test", id: "c1", phase: "done", exit: 0, ms: 900 }) },
    { key: "5", text: JSON.stringify({ action: "write", target: "src/app.ts", id: "c2:0", phase: "done", added: 2, removed: 0 }) },
    { key: "6", text: JSON.stringify({ action: "edit", target: "src/app.ts", id: "c3:0", phase: "done", added: 5, removed: 1 }) },
    { key: "7", text: JSON.stringify({ action: "edit", target: "src/other.ts", id: "c4:0" }) },
    { key: "8", text: JSON.stringify({ action: "read", target: "a.md" }) },
    { key: "9", text: JSON.stringify({ action: "read", target: "a.md" }) },
    { key: "10", text: "not json" },
  ];
  const items = activityItems(rows, row => row.text);
  assert.deepEqual(items.map(item => [item.row.key, item.count]), [["1", 1], ["2", 1], ["3", 2], ["7", 1], ["8", 2]]);
  assert.equal(items[0]!.activity!.exit, 0, "the closing row shows where the command started");
  assert.equal(items[1]!.activity, null, "agent text stays as it is");
  assert.deepEqual([items[2]!.activity!.action, items[2]!.activity!.added, items[2]!.activity!.removed], ["edit", 7, 1], "write then edit of one file: one line");
  // Two first rows with one id (or another file under it): the closing row
  // goes to the matching one only, once, and is never counted twice.
  const clash = activityItems([
    { text: JSON.stringify({ action: "edit", target: "a.ts", id: "c9" }) },
    { text: JSON.stringify({ action: "edit", target: "b.ts", id: "c9" }) },
    { text: JSON.stringify({ action: "edit", target: "b.ts", id: "c9", phase: "done", added: 2, removed: 0 }) },
  ], row => row.text);
  assert.deepEqual(clash.map(item => [item.activity?.target, item.activity?.added]), [["a.ts", undefined], ["b.ts", 2]]);
  // A closing row whose first row left the window still shows.
  const alone = activityItems([{ text: JSON.stringify({ action: "command", target: "ls", id: "c5", phase: "done", exit: 0 }) }], row => row.text);
  assert.equal(alone.length, 1);
});
