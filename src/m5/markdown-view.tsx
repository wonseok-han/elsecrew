import { Fragment, type ReactNode } from "react";
import { useI18n } from "../i18n.tsx";
import { parseMarkdownWithLimits, safeLinkTarget, tooLargeToDraw, type MarkdownBlock, type MarkdownInline } from "./markdown.ts";

/**
 * Draws parsed Markdown as React elements only (never as markup), so no text
 * from a report can become HTML. Links do not navigate: a click asks the app
 * to open the address outside, and the app asks the person first.
 */
export function MarkdownView({ source, renderCode }: { source: string; renderCode?: (text: string, language: string) => ReactNode }) {
  const { t } = useI18n();
  // A very large document is shown as it is: drawing it would hold the screen.
  if (tooLargeToDraw(source)) return <><p className="m5-md-plain-note">{t("m5.markdown.tooLarge")}</p><pre className="m5-md-plain">{source}</pre></>;
  const { blocks, truncated } = parseMarkdownWithLimits(source);
  return <>{blocks.map((block, index) => <Block key={index} block={block} renderCode={renderCode} />)}{truncated && <p className="m5-md-plain-note">{t("m5.markdown.truncated")}</p>}</>;
}

function Block({ block, renderCode }: { block: MarkdownBlock; renderCode?: (text: string, language: string) => ReactNode }): ReactNode {
  switch (block.kind) {
    case "heading": {
      // Report headings sit inside a conversation: keep them small (h4–h6).
      const level = Math.min(6, Math.max(4, block.level + 2));
      const Tag = `h${level}` as "h4" | "h5" | "h6";
      return <Tag className={`m5-md-heading m5-md-h${block.level}`}><Inline items={block.children} /></Tag>;
    }
    case "paragraph": return <p><Inline items={block.children} /></p>;
    case "rule": return <hr className="m5-md-rule" />;
    case "quote": return <blockquote className="m5-md-quote">{block.children.map((child, index) => <Block key={index} block={child} renderCode={renderCode} />)}</blockquote>;
    case "code": return renderCode ? renderCode(block.text, block.language) : <pre className="m5-md-code"><code>{block.text}</code></pre>;
    case "list": {
      const items = block.items.map((item, index) => <li key={index}>{item.map((child, inner) => <Block key={inner} block={child} renderCode={renderCode} />)}</li>);
      return block.ordered ? <ol className="m5-md-list" start={block.start}>{items}</ol> : <ul className="m5-md-list">{items}</ul>;
    }
    case "table": return <div className="m5-md-table" tabIndex={0}><table>
      <thead><tr>{block.header.map((cell, index) => <th key={index} style={{ textAlign: block.align[index] ?? undefined }}><Inline items={cell} /></th>)}</tr></thead>
      <tbody>{block.rows.map((row, index) => <tr key={index}>{row.map((cell, inner) => <td key={inner} style={{ textAlign: block.align[inner] ?? undefined }}><Inline items={cell} /></td>)}</tr>)}</tbody>
    </table></div>;
  }
}

function Inline({ items }: { items: readonly MarkdownInline[] }): ReactNode {
  return <>{items.map((item, index) => <Fragment key={index}>{inline(item)}</Fragment>)}</>;
}

function inline(item: MarkdownInline): ReactNode {
  switch (item.kind) {
    case "text": return item.text;
    case "break": return <br />;
    case "code": return <code className="m5-md-inline-code">{item.text}</code>;
    case "strong": return <strong><Inline items={item.children} /></strong>;
    case "em": return <em><Inline items={item.children} /></em>;
    case "del": return <del><Inline items={item.children} /></del>;
    case "link": return item.href ? <ExternalLink href={item.href}><Inline items={item.children} /></ExternalLink> : <Inline items={item.children} />;
  }
}

/** Asks the app to open an address outside; the app confirms with the person. */
export function openExternalLink(href: string): void {
  const target = safeLinkTarget(href);
  if (!target) return;
  void window.m5ExternalLink?.open(target).catch(() => undefined);
}

function ExternalLink({ href, children }: { href: string; children: ReactNode }) {
  const { t } = useI18n();
  // No href: nothing in the page can navigate or open a window from it.
  return <a className="m5-md-link" role="link" tabIndex={0} title={t("m5.markdown.openLink", { url: href })}
    onClick={event => { event.preventDefault(); event.stopPropagation(); openExternalLink(href); }}
    onKeyDown={event => { if (event.key === "Enter") { event.preventDefault(); event.stopPropagation(); openExternalLink(href); } }}>{children}</a>;
}
