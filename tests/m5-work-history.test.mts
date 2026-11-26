import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { maskSecrets, SECRET_MASK } from "../src/m5/secret-shape.ts";
import { activityDetail, activityItems, activityLine, BLOCK_MAX, durationParts, LINE_MAX, parseWorkActivity, workspacePath } from "../src/m5/work-history.ts";

test("work history rows show the kind of work and never unknown fields", () => {
  assert.deepEqual(parseWorkActivity('{"action":"read","target":"src/app.ts"}'), { action: "read", target: "src/app.ts" });
  assert.deepEqual(parseWorkActivity('{"action":"command","outcome":"refused"}'), { action: "command", outcome: "refused" });
  assert.deepEqual(parseWorkActivity('{"action":"other","tool":"WebFetch"}'), { action: "other", tool: "WebFetch" });
  assert.deepEqual(parseWorkActivity('{"action":"delete","target":"old.md","id":"i:2"}'), { action: "delete", target: "old.md", id: "i:2" });
  for (const text of [
    "plain model text", "[]", "null",
    '{"action":"move"}',
    '{"action":"read","secret":"x"}',
    '{"action":"read","target":""}',
    '{"action":"read","outcome":"maybe"}',
    '{"action":"read","phase":"started"}',
    '{"action":"command","exit":"0"}',
    '{"action":"command","ms":-1}',
    '{"action":"edit","added":1.5}',
    `{"action":"read","target":"${"a".repeat(20000)}"}`,
  ]) assert.equal(parseWorkActivity(text), null, text);
  const emoji = "😀".repeat(100);
  assert.deepEqual(parseWorkActivity(JSON.stringify({ action: "read", target: emoji })), { action: "read", target: emoji });
});

test("control and direction marks are neutralised; lines and blocks are capped", () => {
  assert.equal(parseWorkActivity(JSON.stringify({ action: "read", target: "file\u202etxt.exe" }))!.target, "file\ufffdtxt.exe");
  assert.equal(parseWorkActivity(JSON.stringify({ action: "read", target: "a\nb" }))!.target, "a\ufffdb");
  assert.equal(parseWorkActivity(JSON.stringify({ action: "command", target: "x", command: "echo \u001b[31mred\u0007\nnext\r\nlast\tend" }))!.command, "echo \ufffd[31mred\ufffd\nnext\nlast\tend");
  assert.equal(parseWorkActivity(JSON.stringify({ action: "command", output: "\u2066hidden\u2069 \u200bzero" }))!.output, "\ufffdhidden\ufffd \ufffdzero");
  const longLine = parseWorkActivity(JSON.stringify({ action: "command", target: "x".repeat(400) }))!.target!;
  assert.equal(Array.from(longLine).length, LINE_MAX + 1);
  assert.ok(longLine.endsWith("…"));
  const longBlock = parseWorkActivity(JSON.stringify({ action: "command", output: "y".repeat(9000) }))!.output!;
  assert.equal(Array.from(longBlock).length, BLOCK_MAX + 1);
});

test("secrets are masked with the same rules as the engine (shared cases)", () => {
  // Inputs that look like secrets are stored as hex so secret scanners do not flag the fixture.
  const raw = JSON.parse(readFileSync(new URL("./fixtures/m5/secret-mask-cases.json", import.meta.url), "utf8")) as { text?: string; textHex?: string; masked: string }[];
  const cases = raw.map(({ text, textHex, masked }) => ({ text: text ?? Buffer.from(textHex!, "hex").toString("utf8"), masked }));
  assert.ok(cases.length >= 20);
  for (const { text, masked } of cases) assert.equal(maskSecrets(text), masked, text);
  // Masking twice changes nothing (rows the engine already masked are masked again here).
  for (const { masked } of cases) assert.equal(maskSecrets(masked), masked, masked);
  // A row stored by an older build, unmasked, is masked when shown.
  const old = parseWorkActivity(JSON.stringify({ action: "command", target: "curl -H 'Authorization: Bearer abcdefghijkl' https://api.example.com" }))!;
  assert.equal(old.target, `curl -H 'Authorization: ${SECRET_MASK} ${SECRET_MASK}' https://api.example.com`);
  const output = parseWorkActivity(JSON.stringify({ action: "command", output: "OPENAI_API_KEY=sk-proj-abcdefghijklmnop1234\nPORT=3000" }))!;
  assert.equal(output.output, `OPENAI_API_KEY=${SECRET_MASK}\nPORT=3000`);
});

test("a line names what was worked on; a command folds open to its detail", () => {
  const command = parseWorkActivity(JSON.stringify({ action: "command", target: "npm test", command: "npm test", cwd: ".", exit: 1, ms: 1500, output: "1 failing", outcome: "failed", phase: "done", id: "t" }))!;
  assert.deepEqual(activityLine(command), { label: "m5.output.activity.command", target: "npm test", file: null });
  assert.deepEqual(activityDetail(command), { command: "npm test", cwd: ".", note: null, exit: 1, ms: 1500, lines: null, output: "1 failing" });
  // Only the command, and it is the whole line: nothing to fold.
  assert.equal(activityDetail({ action: "command", target: "ls", command: "ls" }), null);
  assert.equal(activityDetail({ action: "read", target: "src/app.ts" }), null);
  const edit = parseWorkActivity(JSON.stringify({ action: "edit", target: "src/app.ts", added: 3, removed: 1, phase: "done", id: "e" }))!;
  assert.equal(activityLine(edit).file, "src/app.ts");
  assert.deepEqual(activityDetail(edit)!.lines, { added: 3, removed: 1 });
  assert.deepEqual(durationParts(0), { key: "m5.output.activity.instant", values: {} });
  assert.deepEqual(durationParts(400), { key: "m5.output.activity.ms", values: { s: 0.4 } });
  assert.deepEqual(durationParts(12_300), { key: "m5.output.activity.seconds", values: { s: 12 } });
  assert.deepEqual(durationParts(125_000), { key: "m5.output.activity.minutes", values: { m: 2, s: 5 } });
});

test("only a file inside the workspace opens its change", () => {
  assert.equal(workspacePath("src/app.ts"), "src/app.ts");
  for (const target of ["~/notes.txt", "/etc/hosts", "../secret.txt", "src/../../x", ".", "C:\\Users\\x", "a\ufffdb", `${"a".repeat(10)}…`, undefined]) {
    assert.equal(workspacePath(target), null, String(target));
  }
  assert.equal(activityLine({ action: "edit", target: "~/notes.txt" }).file, null);
  assert.equal(activityLine({ action: "read", target: "src/app.ts" }).file, null, "reading changes nothing");
});

test("a tool use's closing row takes its first row's place; same-file edits read as one line", () => {
  const rows = [
    { key: "1", text: JSON.stringify({ action: "command", target: "npm test", command: "npm test", id: "a" }) },
    { key: "2", text: null as string | null },
    { key: "3", text: JSON.stringify({ action: "edit", target: "src/app.ts", id: "b:0" }) },
    { key: "4", text: JSON.stringify({ action: "command", target: "npm test", command: "npm test", id: "a", phase: "done", exit: 0, ms: 900 }) },
    { key: "5", text: JSON.stringify({ action: "edit", target: "src/app.ts", id: "b:0", phase: "done", added: 2, removed: 1 }) },
    { key: "6", text: JSON.stringify({ action: "edit", target: "src/app.ts", id: "c:0", phase: "done", added: 5, removed: 0 }) },
    { key: "7", text: JSON.stringify({ action: "edit", target: "src/other.ts", id: "d:0" }) },
    { key: "8", text: JSON.stringify({ action: "read", target: "a.md" }) },
    { key: "9", text: JSON.stringify({ action: "read", target: "a.md" }) },
    { key: "10", text: "not json" },
  ];
  const items = activityItems(rows, row => row.text);
  assert.deepEqual(items.map(item => [item.row.key, item.count]), [["1", 1], ["2", 1], ["3", 2], ["7", 1], ["8", 2]]);
  assert.equal(items[0]!.activity!.exit, 0, "the closing row shows where the command started");
  assert.equal(items[1]!.activity, null, "agent text stays as it is");
  assert.deepEqual([items[2]!.activity!.added, items[2]!.activity!.removed], [7, 1]);
  // A closing row whose first row left the window still shows.
  const alone = activityItems([{ text: JSON.stringify({ action: "command", target: "ls", id: "z", phase: "done", exit: 0 }) }], row => row.text);
  assert.equal(alone.length, 1);
});
