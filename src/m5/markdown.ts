import { validateExternalLink } from "./contracts/external-link-v1.ts";

/**
 * A small Markdown reader for team reports. Presentation only.
 *
 * Safety (team member names and reports are quoted into the coordinator's
 * summary, so every byte here is untrusted):
 * - HTML is never interpreted: `<b>` stays the literal text "<b>".
 * - The result is a tree of plain values that the view turns into elements;
 *   nothing is ever handed to the page as markup.
 * - A link keeps a target only if it is an absolute http(s) or mailto address
 *   without credentials; anything else (`javascript:`, `data:`, `file:`,
 *   relative paths) becomes plain text. Images show their description only,
 *   so a report can never make the app load something.
 */
export type MarkdownInline =
  | { kind: "text"; text: string }
  | { kind: "code"; text: string }
  | { kind: "strong" | "em" | "del"; children: MarkdownInline[] }
  | { kind: "link"; href: string | null; children: MarkdownInline[] }
  | { kind: "break" };

export type MarkdownBlock =
  | { kind: "heading"; level: number; children: MarkdownInline[] }
  | { kind: "paragraph"; children: MarkdownInline[] }
  | { kind: "list"; ordered: boolean; start: number; items: MarkdownBlock[][] }
  | { kind: "quote"; children: MarkdownBlock[] }
  | { kind: "code"; text: string; language: string }
  | { kind: "table"; align: ("left" | "center" | "right" | null)[]; header: MarkdownInline[][]; rows: MarkdownInline[][][] }
  | { kind: "rule" };

const MAX_DEPTH = 8;

/** The only link targets a report may carry; everything else is shown as text. */
export function safeLinkTarget(raw: string): string | null {
  return validateExternalLink(raw.trim().replace(/^<(.*)>$/s, "$1"));
}

const FENCE = /^ {0,3}(`{3,}|~{3,})(.*)$/;
const HEADING = /^ {0,3}(#{1,6})(?:[ \t]+(.*?))?[ \t]*$/;
const RULE = /^ {0,3}([-*_])(?:[ \t]*\1){2,}[ \t]*$/;
const QUOTE = /^ {0,3}> ?/;
const LIST_ITEM = /^( {0,12})([-*+]|\d{1,9}[.)])(?:[ \t]+(.*)|[ \t]*$)/;
const TABLE_RULE = /^ *\|? *:?-{1,}:? *(?:\| *:?-{1,}:? *)*\|? *$/;

const indentOf = (line: string) => line.length - line.trimStart().length;
const blank = (line: string) => line.trim() === "";

function startsBlock(line: string): boolean {
  return FENCE.test(line) || (HEADING.test(line) && /^ {0,3}#{1,6}(?:[ \t]|$)/.test(line)) || RULE.test(line) || QUOTE.test(line) || LIST_ITEM.test(line);
}

function splitRow(line: string): string[] {
  let row = line.trim();
  if (row.startsWith("|")) row = row.slice(1);
  if (row.endsWith("|") && !row.endsWith("\\|")) row = row.slice(0, -1);
  const cells: string[] = [];
  let cell = "", code = 0;
  for (let i = 0; i < row.length; i++) {
    const c = row[i]!;
    if (c === "\\" && row[i + 1] === "|") { cell += "|"; i++; continue; }
    if (c === "`") code = code ? 0 : 1;
    if (c === "|" && !code) { cells.push(cell.trim()); cell = ""; continue; }
    cell += c;
  }
  cells.push(cell.trim());
  return cells;
}

export function parseMarkdown(source: string, depth = 0): MarkdownBlock[] {
  const lines = source.replace(/\r\n?/g, "\n").split("\n");
  return parseLines(lines, depth);
}

function parseLines(lines: string[], depth: number): MarkdownBlock[] {
  const blocks: MarkdownBlock[] = [];
  let i = 0;
  while (i < lines.length) {
    const line = lines[i]!;
    if (blank(line)) { i++; continue; }
    const fence = FENCE.exec(line);
    if (fence && !(fence[1]![0] === "`" && fence[2]!.includes("`"))) {
      const marker = fence[1]!;
      const body: string[] = [];
      i++;
      while (i < lines.length && !new RegExp(`^ {0,3}${marker[0] === "`" ? "`" : "~"}{${marker.length},}[ \\t]*$`).test(lines[i]!)) body.push(lines[i++]!);
      i++;
      blocks.push({ kind: "code", text: body.join("\n") + (body.length ? "\n" : ""), language: fence[2]!.trim().split(/\s+/)[0] ?? "" });
      continue;
    }
    const heading = /^ {0,3}#{1,6}(?:[ \t]|$)/.test(line) ? HEADING.exec(line) : null;
    if (heading) {
      blocks.push({ kind: "heading", level: heading[1]!.length, children: parseInline((heading[2] ?? "").replace(/[ \t]+#+$/, ""), depth) });
      i++; continue;
    }
    if (RULE.test(line)) { blocks.push({ kind: "rule" }); i++; continue; }
    if (QUOTE.test(line)) {
      const body: string[] = [];
      while (i < lines.length && !blank(lines[i]!) && (QUOTE.test(lines[i]!) || !startsBlock(lines[i]!))) body.push(lines[i++]!.replace(QUOTE, ""));
      blocks.push({ kind: "quote", children: depth < MAX_DEPTH ? parseLines(body, depth + 1) : [{ kind: "paragraph", children: [{ kind: "text", text: body.join(" ") }] }] });
      continue;
    }
    const item = LIST_ITEM.exec(line);
    if (item) {
      const base = item[1]!.length;
      const ordered = /\d/.test(item[2]![0]!);
      const items: string[][] = [];
      while (i < lines.length) {
        const current = lines[i]!;
        const next = LIST_ITEM.exec(current);
        if (next && next[1]!.length === base && /\d/.test(next[2]![0]!) === ordered) {
          items.push([next[3] ?? ""]); i++; continue;
        }
        if (blank(current)) {
          // A blank line continues the list only when more of it follows.
          let ahead = i + 1;
          while (ahead < lines.length && blank(lines[ahead]!)) ahead++;
          const following = lines[ahead];
          const sameList = following !== undefined && (indentOf(following) > base
            || (LIST_ITEM.exec(following)?.[1]!.length === base && /\d/.test(LIST_ITEM.exec(following)![2]![0]!) === ordered));
          if (!sameList) break;
          items[items.length - 1]!.push(""); i++; continue;
        }
        if (indentOf(current) > base) { items[items.length - 1]!.push(current.slice(Math.min(indentOf(current), base + 2 + (ordered ? 1 : 0)))); i++; continue; }
        if (!startsBlock(current) && items[items.length - 1]![items[items.length - 1]!.length - 1] !== "") { items[items.length - 1]!.push(current.trim()); i++; continue; }
        break;
      }
      blocks.push({ kind: "list", ordered, start: ordered ? Number.parseInt(item[2]!, 10) || 1 : 1,
        items: items.map(body => depth < MAX_DEPTH ? parseLines(body, depth + 1) : [{ kind: "paragraph", children: [{ kind: "text", text: body.join(" ") }] }]) });
      continue;
    }
    if (line.includes("|") && i + 1 < lines.length && TABLE_RULE.test(lines[i + 1]!) && lines[i + 1]!.includes("-")) {
      const header = splitRow(line);
      const align = splitRow(lines[i + 1]!).map(cell => cell.startsWith(":") && cell.endsWith(":") ? "center" as const : cell.endsWith(":") ? "right" as const : cell.startsWith(":") ? "left" as const : null);
      i += 2;
      const rows: MarkdownInline[][][] = [];
      while (i < lines.length && !blank(lines[i]!) && lines[i]!.includes("|")) rows.push(splitRow(lines[i++]!).map(cell => parseInline(cell, depth)));
      blocks.push({ kind: "table", align, header: header.map(cell => parseInline(cell, depth)), rows });
      continue;
    }
    const paragraph: string[] = [];
    while (i < lines.length && !blank(lines[i]!) && (paragraph.length === 0 || !startsBlock(lines[i]!))) paragraph.push(lines[i++]!.trim());
    blocks.push({ kind: "paragraph", children: parseInline(paragraph.join("\n"), depth) });
  }
  return blocks;
}

/** Closing bracket for `[` at `open`, counting nested brackets; -1 if none. */
function closeBracket(text: string, open: number): number {
  let level = 0;
  for (let i = open; i < text.length; i++) {
    const c = text[i];
    if (c === "\\") { i++; continue; }
    if (c === "`") { const end = text.indexOf("`", i + 1); if (end > i) { i = end; continue; } }
    if (c === "[") level++;
    else if (c === "]" && --level === 0) return i;
  }
  return -1;
}

/** `(target "title")` right after a link's text; returns target and end. */
function linkDestination(text: string, open: number): { target: string; end: number } | null {
  if (text[open] !== "(") return null;
  let level = 0;
  for (let i = open; i < text.length; i++) {
    const c = text[i];
    if (c === "\\") { i++; continue; }
    if (c === "\n") return null;
    if (c === "(") level++;
    else if (c === ")" && --level === 0) {
      const inside = text.slice(open + 1, i).trim();
      const target = /^<[^>]*>/.exec(inside)?.[0] ?? inside.split(/\s+/)[0] ?? "";
      return { target, end: i + 1 };
    }
  }
  return null;
}

const BARE_URL = /^https?:\/\/[^\s<>()[\]{}"'`]+/;
const ESCAPABLE = /[\\`*_{}[\]()#+\-.!|~<>]/;

export function parseInline(source: string, depth = 0): MarkdownInline[] {
  const out: MarkdownInline[] = [];
  let buffer = "";
  const flush = () => { if (buffer) { out.push({ kind: "text", text: buffer }); buffer = ""; } };
  const nested = (text: string) => depth < MAX_DEPTH ? parseInline(text, depth + 1) : [{ kind: "text" as const, text }];
  let i = 0;
  while (i < source.length) {
    const c = source[i]!;
    if (c === "\\" && i + 1 < source.length && ESCAPABLE.test(source[i + 1]!)) { buffer += source[i + 1]; i += 2; continue; }
    if (c === "\n") { flush(); out.push({ kind: "break" }); i++; continue; }
    if (c === "`") {
      let run = 1; while (source[i + run] === "`") run++;
      const fence = "`".repeat(run);
      let end = source.indexOf(fence, i + run);
      while (end >= 0 && source[end + run] === "`") end = source.indexOf(fence, end + run + 1);
      if (end > 0) {
        flush();
        const text = source.slice(i + run, end).replace(/\n/g, " ");
        out.push({ kind: "code", text: /^ .* $/.test(text) && text.trim() ? text.slice(1, -1) : text });
        i = end + run; continue;
      }
      buffer += fence; i += run; continue;
    }
    if (c === "<") {
      const auto = /^<((?:https?|mailto):[^\s<>]+)>/i.exec(source.slice(i));
      if (auto) { flush(); out.push({ kind: "link", href: safeLinkTarget(auto[1]!), children: [{ kind: "text", text: auto[1]! }] }); i += auto[0].length; continue; }
    }
    if ((c === "[" || (c === "!" && source[i + 1] === "[")) && depth < MAX_DEPTH) {
      const open = c === "!" ? i + 1 : i;
      const close = closeBracket(source, open);
      const destination = close > 0 ? linkDestination(source, close + 1) : null;
      if (destination) {
        flush();
        const label = source.slice(open + 1, close);
        // An image shows its description only: a report never makes the app load anything.
        if (c === "!") out.push(...nested(label));
        else out.push({ kind: "link", href: safeLinkTarget(destination.target), children: nested(label) });
        i = destination.end; continue;
      }
    }
    if (c === "*" || c === "_" || c === "~") {
      let run = 1; while (source[i + run] === c) run++;
      const size = c === "~" ? (run >= 2 ? 2 : 0) : Math.min(run, 3);
      const before = source[i - 1] ?? " ";
      const after = source[i + run] ?? " ";
      const opens = size > 0 && !/\s/.test(after) && (c !== "_" || !/[\p{L}\p{N}]/u.test(before));
      if (opens) {
        const marker = c.repeat(size);
        let end = i + run;
        let found = -1;
        while ((end = source.indexOf(marker, end)) > 0) {
          const prior = source[end - 1]!;
          const following = source[end + size] ?? " ";
          if (!/\s/.test(prior) && source[end + size] !== c && (c !== "_" || !/[\p{L}\p{N}]/u.test(following))) { found = end; break; }
          end += 1;
        }
        if (found > i + size) {
          flush();
          const children = nested(source.slice(i + size, found));
          out.push(c === "~" ? { kind: "del", children }
            : size === 1 ? { kind: "em", children }
            : size === 2 ? { kind: "strong", children }
            : { kind: "strong", children: [{ kind: "em", children }] });
          i = found + size; continue;
        }
      }
      buffer += c.repeat(run); i += run; continue;
    }
    if ((c === "h" || c === "H") && /[^\p{L}\p{N}]|^$/u.test(source[i - 1] ?? "")) {
      const bare = BARE_URL.exec(source.slice(i));
      if (bare) {
        const url = bare[0].replace(/[.,;:!?]+$/, "");
        flush();
        out.push({ kind: "link", href: safeLinkTarget(url), children: [{ kind: "text", text: url }] });
        i += url.length; continue;
      }
    }
    buffer += c; i++;
  }
  flush();
  return out;
}

/** Plain words of inline content (no markup characters). */
export function inlineText(inline: readonly MarkdownInline[]): string {
  return inline.map(item => item.kind === "text" || item.kind === "code" ? item.text
    : item.kind === "break" ? " "
    : inlineText(item.children)).join("");
}

/** The report's sentences in reading order, for a short summary: prose only. */
export function proseText(blocks: readonly MarkdownBlock[]): string[] {
  const out: string[] = [];
  for (const block of blocks) {
    if (block.kind === "paragraph") out.push(inlineText(block.children));
    else if (block.kind === "list") for (const item of block.items) out.push(...proseText(item));
    else if (block.kind === "quote") out.push(...proseText(block.children));
  }
  return out.map(text => text.replace(/\s+/g, " ").trim()).filter(Boolean);
}
