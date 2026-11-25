import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { runInNewContext } from "node:vm";
import test from "node:test";
import ts from "typescript";
import { renderToStaticMarkup } from "react-dom/server";
import { parseInline, parseMarkdown, safeLinkTarget } from "../src/m5/markdown.ts";
import * as markdown from "../src/m5/markdown.ts";
import { messageSummary } from "../src/m5/message-summary.ts";
import { correctSettingsDraft, editSettingsDraft, loadSettingsDraft, settingsDraftDirty } from "../src/m5/settings-draft.ts";
import { timelineMessages } from "../src/m5/timeline-messages.ts";
import { activityLine, collapseRepeats } from "../src/m5/work-history.ts";
import { forgetLastProject, recallLastProject, rememberLastProject, shouldResumeProject } from "../src/m5/last-session.ts";
import { NotificationCenter } from "../src/m5/notifications.ts";
import { startNotificationFeed } from "../src/m5/notification-feed.ts";
import { validateExternalLink } from "../src/m5/contracts/external-link-v1.ts";
import { ko } from "../src/locales/ko.ts";

const require = createRequire(import.meta.url);

function renderMarkdown(source: string): string {
  const compiled = ts.transpileModule(readFileSync(new URL("../src/m5/markdown-view.tsx", import.meta.url), "utf8"), {
    compilerOptions: { module: ts.ModuleKind.CommonJS, jsx: ts.JsxEmit.ReactJSX, target: ts.ScriptTarget.ES2020 },
  }).outputText;
  const context: any = { exports: {}, require(name: string) {
    if (name === "../i18n.tsx") return { useI18n: () => ({ t: (key: string) => key }) };
    if (name === "./markdown.ts") return markdown;
    return require(name);
  } };
  runInNewContext(compiled, context);
  return renderToStaticMarkup(context.exports.MarkdownView({ source }));
}

test("role reports render as text, with no raw Markdown marks left (D2)", () => {
  const report = [
    "## 결과 요약",
    "",
    "**변경 파일**: `index.html`, `style.css`",
    "",
    "- 오늘 날짜를 *작게* 표시",
    "- 확인:",
    "  1. 새로고침",
    "  2. 체크",
    "",
    "| 항목 | 결과 |",
    "| --- | :---: |",
    "| 추가 | 통과 |",
    "",
    "> 참고: ~~옛 방식~~ 대신",
    "",
    "---",
  ].join("\n");
  const html = renderMarkdown(report);
  const text = html.replace(/<[^>]+>/g, "");
  for (const mark of ["##", "**", "`", "| ---", "~~", "> 참고", "---"]) assert.ok(!text.includes(mark), `${mark} in ${text}`);
  assert.ok(html.includes("<strong>변경 파일</strong>"));
  assert.ok(html.includes("<em>작게</em>"));
  assert.ok(html.includes("<table>") && html.includes("통과"));
  assert.ok(html.includes("<ol") && html.includes("새로고침"));
  assert.ok(html.includes("<del>옛 방식</del>"));
});

test("the renderer never draws HTML or dangerous links, even from a team member's name", () => {
  const hostile = [
    "<img src=x onerror=alert(1)> <script>alert(1)</script>",
    "[누르기](javascript:alert(1)) [data](data:text/html,<b>x</b>) [file](file:///etc/passwd)",
    "[creds](https://user:pw@example.com) [rel](/relative) ![pic](https://example.com/a.png)",
    "**<b>이름</b>**",
  ].join("\n\n");
  const html = renderMarkdown(hostile);
  assert.ok(!/<(img|script|b)[\s>]/i.test(html), html);
  assert.ok(html.includes("&lt;script&gt;"));
  assert.ok(!/href=|javascript:|onerror=|src=/i.test(html.replace(/&lt;[^&]*&gt;/g, "")), html);
  // An image shows only its description; nothing is loaded.
  assert.ok(html.includes("pic") && !html.includes("<img"));
  // A safe link is drawn without an address the page could follow.
  const safe = renderMarkdown("[문서](https://example.com/docs) https://example.com/b.");
  assert.ok(safe.includes("m5-md-link") && !safe.includes("href="));
  for (const bad of ["javascript:alert(1)", "JAVASCRIPT:alert(1)", " javascript:alert(1)", "data:text/html,x", "vbscript:x", "https://u:p@x.com", "//evil.com", "https://exa‮mple.com"]) {
    assert.equal(safeLinkTarget(bad), null, bad);
    assert.equal(validateExternalLink(bad), null, bad);
  }
  assert.equal(safeLinkTarget("https://example.com/a?b=1"), "https://example.com/a?b=1");
  const inline = parseInline("[x](javascript:alert(1))");
  assert.deepEqual(inline, [{ kind: "link", href: null, children: [{ kind: "text", text: "x" }] }]);
});

test("Markdown blocks: headings, lists, nested items, fences and escapes", () => {
  const blocks = parseMarkdown("# 제목\n\n1. 하나\n2. 둘\n   - 안쪽\n\n```js\nconst a = 1;\n```\n\n\\*별\\* 그대로");
  assert.deepEqual(blocks.map(block => block.kind), ["heading", "list", "code", "paragraph"]);
  const list = blocks[1] as Extract<typeof blocks[number], { kind: "list" }>;
  assert.equal(list.ordered, true);
  assert.equal(list.items.length, 2);
  assert.equal(list.items[1]![1]!.kind, "list");
  assert.deepEqual((blocks[3] as { children: unknown[] }).children, [{ kind: "text", text: "*별* 그대로" }]);
  // Deeply nested input stays bounded.
  assert.doesNotThrow(() => parseMarkdown("> ".repeat(500) + "끝"));
  assert.doesNotThrow(() => parseInline("*".repeat(5000) + "x"));
});

test("long reports fold to their first sentences; short ones show whole", () => {
  assert.equal(messageSummary("짧은 답이에요.").long, false);
  const long = "## 결과\n\n오늘 날짜를 목록 아래에 표시했어요. 기존 기능은 그대로예요. 확인 순서는 아래와 같아요. 네 번째 문장은 접혀요.\n\n" + "- 세부 항목\n".repeat(20);
  const summary = messageSummary(long);
  assert.equal(summary.long, true);
  assert.equal(summary.summary, "오늘 날짜를 목록 아래에 표시했어요. 기존 기능은 그대로예요. 확인 순서는 아래와 같아요.");
  assert.ok(!/[#*`|]/.test(summary.summary));
});

test("opening settings and closing them is not an unsaved change; the person's edits are (D12)", () => {
  type Team = { roles: { roleId: string; model: string }[] };
  const same = (a: Team, b: Team) => JSON.stringify(a) === JSON.stringify(b);
  const loaded: Team = { roles: [{ roleId: "coordinator", model: "" }, { roleId: "coding", model: "opus" }] };
  let draft = loadSettingsDraft(loaded);
  assert.equal(settingsDraftDirty(draft, same), false);
  // The screen fills a missing model with the provider's recommendation by itself.
  draft = correctSettingsDraft(draft, team => ({ roles: team.roles.map(role => role.model ? role : { ...role, model: "recommended" }) }));
  assert.equal(draft.current.roles[0]!.model, "recommended");
  assert.equal(settingsDraftDirty(draft, same), false, "an automatic correction is not the person's change");
  draft = editSettingsDraft(draft, team => ({ roles: team.roles.map(role => role.roleId === "coding" ? { ...role, model: "sonnet" } : role) }));
  assert.equal(settingsDraftDirty(draft, same), true);
  // A correction after an edit keeps the edit dirty.
  draft = correctSettingsDraft(draft, team => team);
  assert.equal(settingsDraftDirty(draft, same), true);
  draft = editSettingsDraft(draft, team => ({ roles: team.roles.map(role => role.roleId === "coding" ? { ...role, model: "opus" } : role) }));
  assert.equal(settingsDraftDirty(draft, same), false, "changing it back is not a change");
});

test("the role editor uses the shared draft rule, not its own comparison", () => {
  const source = readFileSync(new URL("../src/m5/workbench.tsx", import.meta.url), "utf8");
  const editor = source.slice(source.indexOf("function RoleSettings("), source.indexOf("function FollowupComposer("));
  assert.ok(editor.includes("settingsDraftDirty(draft, sameTeam)"));
  assert.ok(editor.includes("correctTeam(old =>"), "model defaults are corrections");
  assert.ok(!editor.includes("!sameTeam(team, initial)"));
});

test("the user's goal shows once and filler status lines are left out (D4①②)", () => {
  const messages = [
    { kind: "user", text: "날짜를 보여 줘", roleId: null },
    { kind: "role", text: "계획", roleId: "coordinator" },
    { kind: "status", text: "팀 작업을 이어가고 있어요.", roleId: null },
    { kind: "status", text: "답변을 마쳤어요.", roleId: null },
    { kind: "user", text: "날짜를 보여 줘", roleId: null },
  ];
  assert.deepEqual(timelineMessages(messages, "날짜를 보여 줘").map(message => message.text), ["계획", "답변을 마쳤어요.", "날짜를 보여 줘"]);
  assert.equal(timelineMessages(messages, null).length, 4);
});

test("activity lines read as a Korean verb and a place; a command's own words fold (D6)", () => {
  assert.deepEqual(activityLine({ action: "read", target: "src/app.ts", tool: "Read" }), { label: "m5.output.activity.read", target: "src/app.ts", original: null });
  assert.deepEqual(activityLine({ action: "command", target: "List files and view html/css", tool: "Bash" }), { label: "m5.output.activity.command", target: null, original: "List files and view html/css" });
  assert.equal(activityLine({ action: "search", tool: "Grep", target: "today" }).label, "m5.output.activity.searchText");
  assert.equal(ko["m5.output.activity.read"], "파일 읽기");
  assert.deepEqual(collapseRepeats(["a", "a", "b", "a"], (x, y) => x === y), [{ row: "a", count: 2 }, { row: "b", count: 1 }, { row: "a", count: 1 }]);
});

test("restart reopens the last project once; a closed or removed project stays closed (F13)", () => {
  const saved = new Map<string, string>();
  const storage = { getItem: (key: string) => saved.get(key) ?? null, setItem: (key: string, value: string) => { saved.set(key, value); }, removeItem: (key: string) => { saved.delete(key); } };
  rememberLastProject(storage, "project_1");
  assert.equal(recallLastProject(storage), "project_1");
  const base = { remembered: "project_1", attempted: false, ready: true, projectOpen: false, busy: false, known: ["project_1", "project_2"] };
  assert.equal(shouldResumeProject(base), "project_1");
  assert.equal(shouldResumeProject({ ...base, attempted: true }), null);
  assert.equal(shouldResumeProject({ ...base, ready: false }), null);
  assert.equal(shouldResumeProject({ ...base, projectOpen: true }), null);
  assert.equal(shouldResumeProject({ ...base, known: ["project_2"] }), null);
  assert.equal(shouldResumeProject({ ...base, known: null }), null);
  forgetLastProject(storage);
  assert.equal(recallLastProject(storage), null);
  rememberLastProject(storage, "../../etc");
  assert.equal(recallLastProject(storage), null);
});

test("a permission notification settles once its request goes away (D13)", async () => {
  const center = new NotificationCenter(null);
  const listeners: ((event: any) => void)[] = [];
  let pending = [{ approvalId: "approval_1" }];
  const bridge = {
    onAgentRunStateChanged: (listener: (event: any) => void) => { listeners.push(listener); return () => {}; },
    getChat: async () => ({ chat: { chatId: "chat_1", projectId: "project_1", goal: "목표", state: "running" } }),
  };
  const approvals: { request: (input?: unknown) => Promise<unknown> } = { request: async () => ({ pending }) };
  const realSetInterval = globalThis.setInterval;
  let tick: (() => void) | null = null;
  (globalThis as any).setInterval = (fn: () => void) => { tick = fn; return 1; };
  const stop = startNotificationFeed(bridge, center, approvals, () => {});
  (globalThis as any).setInterval = realSetInterval;
  const run = { runId: "run_1", chatId: "chat_1", taskId: "task_1", projectId: "project_1", worktreeId: "w", workspaceIdentity: "i", state: "running", selectedProfile: { roleId: "review" } };
  listeners[0]!({ run });
  tick!();
  await new Promise(resolve => setTimeout(resolve, 10));
  assert.equal(center.snapshot()[0]?.id, "approval:approval_1");
  assert.equal(center.snapshot()[0]?.resolved, undefined);
  pending = [];
  tick!();
  await new Promise(resolve => setTimeout(resolve, 10));
  assert.equal(center.snapshot()[0]?.resolved, true);
  assert.equal(center.unread(), 0);
  // A second request whose slow answer arrives after the run ended stays settled.
  pending = [{ approvalId: "approval_2" }];
  let answer: (value: unknown) => void = () => {};
  approvals.request = () => new Promise(resolve => { answer = resolve; });
  tick!();
  listeners[0]!({ run: { ...run, state: "succeeded" } });
  answer({ pending });
  await new Promise(resolve => setTimeout(resolve, 10));
  assert.ok(!center.snapshot().some(item => item.id === "approval:approval_2"), "an out-of-date answer adds nothing");
  stop();
});

test("a task's permission notices settle together once it no longer works", () => {
  const center = new NotificationCenter(null);
  for (const id of ["a", "b"]) center.add({ id: `approval:${id}`, kind: "attention", projectId: "p", chatId: "chat_1", goal: "g", text: "", at: "2026-11-25T00:00:00Z" });
  center.add({ id: "approval:c", kind: "attention", projectId: "p", chatId: "chat_2", goal: "g", text: "", at: "2026-11-25T00:00:00Z" });
  center.add({ id: "done:run", kind: "done", projectId: "p", chatId: "chat_1", goal: "g", text: "", at: "2026-11-25T00:00:00Z" });
  center.resolveApprovalsFor("chat_1");
  const byId = new Map(center.snapshot().map(item => [item.id, item]));
  assert.equal(byId.get("approval:a")?.resolved, true);
  assert.equal(byId.get("approval:b")?.resolved, true);
  assert.equal(byId.get("approval:c")?.resolved, undefined, "another task's request stays");
  assert.equal(byId.get("done:run")?.resolved, undefined, "only permission requests settle");
});
