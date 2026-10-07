import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import config from "../electron/vite.renderer.config.ts";

test("dev and preview deliver the header-only framing restriction", () => {
  assert.equal(config.server?.headers?.["Content-Security-Policy"], "frame-ancestors 'none'");
  assert.equal(config.preview?.headers?.["Content-Security-Policy"], "frame-ancestors 'none'");
});

test("HTML retains its resource policy without an ignored framing directive", () => {
  const html = readFileSync(new URL("../src/m5/index.html", import.meta.url), "utf8");
  const policy = html.match(/http-equiv="Content-Security-Policy"\s+content="([^"]+)"/)?.[1];
  assert.equal(policy, "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; font-src 'self'; worker-src 'self' blob:; connect-src 'self'; object-src 'none'; base-uri 'none'");
});
