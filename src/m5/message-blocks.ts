export type MessageBlock = { kind: "text"; text: string } | { kind: "code"; text: string; language: string };

// Presentation only: preserve source bytes inside fences; never interpret HTML or URLs.
export function messageBlocks(source: string): MessageBlock[] {
  const blocks: MessageBlock[] = [];
  const opening = /^ {0,3}(`{3,}|~{3,})([^\r\n]*)(?:\r?\n|$)/gm;
  let cursor = 0;
  let match: RegExpExecArray | null;
  while ((match = opening.exec(source))) {
    const fence = match[1];
    if (fence[0] === "`" && match[2].includes("`")) continue;
    if (match.index > cursor) blocks.push({ kind: "text", text: source.slice(cursor, match.index) });
    const contentStart = opening.lastIndex;
    const closing = new RegExp(`^ {0,3}${fence[0]}{${fence.length},}[ \\t]*(?:\\r?\\n|$)`, "gm");
    closing.lastIndex = contentStart;
    const end = closing.exec(source);
    blocks.push({ kind: "code", language: match[2].trim().split(/\s+/)[0] ?? "", text: source.slice(contentStart, end?.index ?? source.length) });
    cursor = end ? closing.lastIndex : source.length;
    opening.lastIndex = cursor;
    if (cursor === source.length) break;
  }
  if (cursor < source.length || blocks.length === 0) blocks.push({ kind: "text", text: source.slice(cursor) });
  return blocks;
}
