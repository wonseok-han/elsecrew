#!/usr/bin/env node
// Work-log entries are moved, never lost or rewritten (AGENTS.md/CLAUDE.md
// "아카이브 롤오버"). Compares every `### YYYY-MM-DD …` entry of a base
// revision (docs/history.md + docs/archive/*.md) with the working tree: each
// one must still exist, word for word (trailing blank lines aside), in one
// of those files. New entries are allowed.
//
//   node scripts/m5-history-entries.mjs [base-ref] [--tree <ref>]
//     base-ref defaults to origin/feature/ade; --tree compares a revision
//     instead of the working tree.
//
// Exit 0: nothing lost or changed; 1: something was; 2: base not readable.
import { execFileSync } from "node:child_process";
import { readdirSync, readFileSync, existsSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const args = process.argv.slice(2);
const treeAt = args.indexOf("--tree");
const tree = treeAt >= 0 ? args.splice(treeAt, 2)[1] : null;
const base = args[0] || "origin/feature/ade";
const git = (...args) => execFileSync("/usr/bin/git", args, { cwd: root, encoding: "utf8", maxBuffer: 64 * 1024 * 1024 });

/** The entries of one document: from a `### YYYY-MM-DD` heading to the next entry or `## ` section. */
export function entries(text) {
  const found = [];
  let current = null;
  for (const line of text.split("\n")) {
    if (/^### \d{4}-\d{2}-\d{2}\b/.test(line)) { if (current) found.push(current); current = [line]; continue; }
    if (/^## /.test(line)) { if (current) found.push(current); current = null; continue; }
    if (current) current.push(line);
  }
  if (current) found.push(current);
  return found.map(lines => lines.join("\n").replace(/\s+$/, ""));
}

function files(read, list) {
  return ["docs/history.md", ...list().filter(name => /^history-.*\.md$/.test(name)).map(name => `docs/archive/${name}`)].flatMap(path => {
    const text = read(path);
    return text === null ? [] : entries(text).map(body => ({ path, body }));
  });
}

let before;
try {
  before = files(path => { try { return git("show", `${base}:${path}`); } catch { return null; } },
    () => git("ls-tree", "--name-only", `${base}:docs/archive`).trim().split("\n"));
} catch (error) {
  console.error(`cannot read ${base}: ${error.message}`);
  process.exit(2);
}
const fromRef = ref => [path => { try { return git("show", `${ref}:${path}`); } catch { return null; } }, () => git("ls-tree", "--name-only", `${ref}:docs/archive`).trim().split("\n")];
const after = tree ? files(...fromRef(tree))
  : files(path => existsSync(join(root, path)) ? readFileSync(join(root, path), "utf8") : null, () => readdirSync(join(root, "docs/archive")));
const now = new Map();
for (const entry of after) now.set(entry.body, (now.get(entry.body) ?? 0) + 1);
const missing = [];
for (const entry of before) {
  const left = now.get(entry.body) ?? 0;
  if (left > 0) now.set(entry.body, left - 1); else missing.push({ from: entry.path, heading: entry.body.split("\n")[0].slice(0, 140) });
}
const added = after.length - (before.length - missing.length);
console.log(JSON.stringify({ base, tree: tree ?? "working tree", before: before.length, after: after.length, kept: before.length - missing.length, added, missing }, null, 1));
process.exit(missing.length === 0 ? 0 : 1);
