const MiB = 1024 * 1024;
function sizedLines(bytes, lines) {
  const line = "가" + "x".repeat(Math.floor(bytes / lines) - 4) + "\n";
  const prefix = line.repeat(lines - 1);
  return prefix + "x".repeat(bytes - Buffer.byteLength(prefix) - 1) + "\n";
}
let cachedFileText;
function getFileText() { return cachedFileText ??= sizedLines(5 * MiB, 100000); }
const patchBytes = 10 * MiB;
function patch(index) {
  const size = Math.floor(patchBytes / 500) + (index < patchBytes % 500 ? 1 : 0);
  const header = `--- a/file-${index}.txt\n+++ b/file-${index}.txt\n@@ -1,20 +1,20 @@\n`;
  const lines = Array.from({ length: 40 }, (_, i) => `${i < 20 ? "-" : "+"}${"x".repeat(500)}\n`).join("");
  const body = header + lines;
  // Extend one changed line; preserve exactly forty changed lines per file.
  return body.slice(0, -1) + "x".repeat(size - Buffer.byteLength(body)) + "\n";
}
function tree(parentPath = "", cursor = null) {
  if (parentPath) return { projectId: "smoke_project", parentPath, entries: Array.from({ length: 10 }, (_, i) => ({ name: i === 0 ? "large.txt" : `small-${i}.txt`, path: `${parentPath}/${i === 0 ? "large.txt" : `small-${i}.txt`}`, kind: "file" })), nextCursor: null };
  const start = cursor === null ? 0 : Number(cursor);
  if (![0, 500].includes(start)) throw new Error("invalid fixture cursor");
  return { projectId: "smoke_project", parentPath, entries: Array.from({ length: 500 }, (_, i) => ({ name: `folder-${start + i}`, path: `folder-${start + i}`, kind: "directory" })), nextCursor: start === 0 ? "500" : null };
}
module.exports = { get fileText() { return getFileText(); }, getFileText, patch, tree };
