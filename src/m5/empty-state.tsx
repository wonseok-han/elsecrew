// One frame for "nothing here yet" (ADE DESIGN 2.1 rule 7, mockup 15): a round
// mark · what is missing · why, or what will appear · at most one button.
import type { ReactNode } from "react";

const ICONS = {
  spark: "M12 3v3M12 18v3M3 12h3M18 12h3M5.6 5.6l2.1 2.1M16.3 16.3l2.1 2.1M5.6 18.4l2.1-2.1M16.3 7.7l2.1-2.1",
  file: "M14 3H7a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8zM14 3v5h5",
  search: "M11 18a7 7 0 1 0 0-14 7 7 0 0 0 0 14zM20 20l-4-4",
  list: "M8 6h12M8 12h12M8 18h12M4 6h.01M4 12h.01M4 18h.01",
  clock: "M12 21a9 9 0 1 0 0-18 9 9 0 0 0 0 18zM12 7v5l3 2",
} as const;
export type EmptyIcon = keyof typeof ICONS;

export function EmptyState({ icon = "spark", title, help, action, children, className = "", headingLevel = 3, label }: {
  icon?: EmptyIcon; title: ReactNode; help?: ReactNode; action?: ReactNode; children?: ReactNode;
  className?: string; headingLevel?: 2 | 3; label?: string;
}) {
  const Heading = headingLevel === 2 ? "h2" : "h3";
  return <section className={`m5-empty-state ${className}`.trim()} aria-label={label}>
    <span className="m5-empty-mark" aria-hidden="true"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round"><path d={ICONS[icon]} /></svg></span>
    <Heading>{title}</Heading>
    {help && <p>{help}</p>}
    {action}
    {children}
  </section>;
}
