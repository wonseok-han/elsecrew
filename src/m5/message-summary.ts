import { parseMarkdown, proseText } from "./markdown.ts";

/** Reports longer than this fold to a short summary with "more". */
const LONG_CHARS = 480;
const LONG_LINES = 8;
const SUMMARY_SENTENCES = 3;
const SUMMARY_CHARS = 260;
const SUMMARY_SOURCE_CHARS = 6_000;

export interface MessageSummary {
  /** Fold the message: show `summary` first, the whole report on request. */
  long: boolean;
  /** The first two or three sentences as plain words (no Markdown marks). */
  summary: string;
}

/** Sentence ends: Latin punctuation and Korean endings followed by space. */
function sentences(text: string): string[] {
  return text.split(/(?<=[.!?…。])\s+/u).map(part => part.trim()).filter(Boolean);
}

/**
 * A long team report reads as its first few sentences; the rest folds under
 * "more". Short messages show whole. Headings, tables and code never make the
 * summary, so it is plain prose.
 */
export function messageSummary(text: string): MessageSummary {
  const lines = text.split("\n").filter(line => line.trim()).length;
  const long = text.length > LONG_CHARS || lines > LONG_LINES;
  if (!long) return { long: false, summary: "" };
  const picked: string[] = [];
  let length = 0;
  // Only the start is read for the summary: a long report never parses whole
  // just to show its first sentences.
  for (const paragraph of proseText(parseMarkdown(text.slice(0, SUMMARY_SOURCE_CHARS)))) {
    for (const sentence of sentences(paragraph)) {
      if (picked.length >= SUMMARY_SENTENCES || (picked.length > 0 && length + sentence.length > SUMMARY_CHARS)) break;
      picked.push(sentence);
      length += sentence.length;
    }
    if (picked.length >= SUMMARY_SENTENCES || length >= SUMMARY_CHARS) break;
  }
  let summary = picked.join(" ");
  if (Array.from(summary).length > SUMMARY_CHARS) summary = `${Array.from(summary).slice(0, SUMMARY_CHARS - 1).join("")}…`;
  // Nothing prose-like (a table or code only): show it whole.
  return summary ? { long: true, summary } : { long: false, summary: "" };
}
