import { useEffect, useMemo, useRef, useState } from "react";
import { useI18n } from "../i18n.tsx";
import { MarkdownView } from "./markdown-view.tsx";
import { messageSummary } from "./message-summary.ts";

/**
 * A message as readable text: Markdown is drawn (never shown as raw marks)
 * and code keeps its copy button. With `fold`, a long report shows its first
 * sentences and opens fully on "more".
 */
export function MessageBody({ text, fold = false }: { text: string; fold?: boolean }) {
  const { t } = useI18n();
  const summary = useMemo(() => fold ? messageSummary(text) : { long: false, summary: "" }, [text, fold]);
  const [open, setOpen] = useState(false);
  useEffect(() => { setOpen(false); }, [text]);
  const full = <div className="m5-message-body m5-md"><MarkdownView source={text} renderCode={(code, language) => <CodeSnippet text={code} language={language} />} /></div>;
  if (!summary.long) return full;
  return <div className="m5-message-fold">
    {!open && <p className="m5-message-summary">{summary.noProse ? t("m5.message.noSummary") : summary.summary}</p>}
    {open && full}
    <button type="button" className="m5-link-button m5-message-more" aria-expanded={open} onClick={() => setOpen(value => !value)}>{t(open ? "m5.message.less" : "m5.message.more")}</button>
  </div>;
}

export function CodeSnippet({ text, language }: { text: string; language: string }) {
  const { t } = useI18n();
  const [status, setStatus] = useState<"idle" | "copied" | "failed">("idle");
  const generation = useRef(0);
  useEffect(() => { generation.current++; setStatus("idle"); return () => { generation.current++; }; }, [text]);
  const copy = async () => {
    const epoch = ++generation.current;
    try {
      await navigator.clipboard.writeText(text);
      if (generation.current === epoch) setStatus("copied");
    } catch {
      if (generation.current === epoch) setStatus("failed");
    }
  };
  return <section className="m5-message-code" aria-label={t("m5.chat.code")}>
    <header><span>{language || t("m5.chat.code")}</span><button type="button" className="m5-secondary" onClick={() => void copy()}>{t("m5.chat.copyCode")}</button></header>
    <pre tabIndex={0}><code>{text}</code></pre>
    {status !== "idle" && <small role="status">{t(status === "copied" ? "m5.chat.codeCopied" : "m5.chat.codeCopyFailed")}</small>}
  </section>;
}
