import { useEffect, useMemo, useRef, useState } from "react";
import { useI18n } from "../i18n.tsx";
import { messageBlocks } from "./message-blocks.ts";

export function MessageBody({ text }: { text: string }) {
  const blocks = useMemo(() => messageBlocks(text), [text]);
  return <div className="m5-message-body">{blocks.map((block, index) => block.kind === "text"
    ? <p key={index}>{block.text}</p>
    : <CodeSnippet key={index} text={block.text} language={block.language} />)}</div>;
}

function CodeSnippet({ text, language }: { text: string; language: string }) {
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
