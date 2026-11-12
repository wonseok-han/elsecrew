import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

async function render() {
  const workerUrl = new URL("../dist/server/index.js", import.meta.url);
  workerUrl.searchParams.set("test", `${process.pid}-${Date.now()}`);
  const { default: worker } = await import(workerUrl.href);

  return worker.fetch(
    new Request("https://elsecrew.test/", {
      headers: { accept: "text/html", host: "elsecrew.test" },
    }),
    {
      ASSETS: {
        fetch: async () => new Response("Not found", { status: 404 }),
      },
    },
    {
      waitUntil() {},
      passThroughOnException() {},
    },
  );
}

test("renders the Elsecrew landing page", async () => {
  const response = await render();
  assert.equal(response.status, 200);
  assert.match(response.headers.get("content-type") ?? "", /^text\/html\b/i);

  const html = await response.text();
  assert.match(html, /<html[^>]*lang="ko"/i);
  assert.match(html, /Elsecrew/);
  assert.match(html, /터미널을 몰라도/);
  assert.match(html, /버튼을 따라가는 6단계/);
  assert.match(html, /내 코딩 작업의 쉬운 출발점/);
  assert.match(html, /프로젝트를 알아서 찾아요/);
  assert.match(html, /새 버전도 놓치지 않아요/);
  assert.match(html, /Claude Code/);
  assert.match(html, /Codex/);
  assert.match(html, /자주 묻는 질문/);
  assert.match(html, /github\.com\/wonseok-han\/hello-agent/);
  assert.match(html, /https:\/\/elsecrew\.test\/og\.png/);
  assert.doesNotMatch(html, /codex-preview|Your site is taking shape/);
});

test("keeps section navigation available while scrolling", async () => {
  const css = await readFile(
    new URL("../app/globals.css", import.meta.url),
    "utf8",
  );

  assert.match(css, /\.site-header\s*\{[^}]*position:\s*sticky/s);
  assert.match(css, /\.site-header\s*\{[^}]*top:\s*0/s);
  assert.match(css, /scroll-padding-top:\s*98px/);
  assert.doesNotMatch(css, /@media \(max-width: 980px\)[\s\S]*?nav\s*\{[^}]*display:\s*none/);
});
