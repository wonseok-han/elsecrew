// Settings building blocks (ADE screen unification U4, DESIGN 2.3): a group of
// setting rows, each with its title, a line of help and its control on the
// right; a row can open below for more (a list, a check). Every settings page
// is made of these. The old class names stay on the rows (`m5-preferences-row`)
// so older checks and styles keep finding them.
import type { ReactNode } from "react";

export function SettingsGroup({ label, children, className = "", testId }: { label?: ReactNode; children: ReactNode; className?: string; testId?: string }) {
  return <>
    {label && <h4 className="m5-group-label">{label}</h4>}
    <div className={`m5-group m5-settings-group ${className}`.trim()} data-testid={testId}>{children}</div>
  </>;
}

export function Setting({ title, help, control, children, className = "", titleId, as = "div" }: {
  title: ReactNode; help?: ReactNode; control?: ReactNode; children?: ReactNode; className?: string; titleId?: string; as?: "div" | "label";
}) {
  const Row = as;
  return <Row className={`m5-setting m5-preferences-row ${className}`.trim()}>
    <span className="m5-setting-text"><strong id={titleId}>{title}</strong>{help && <small>{help}</small>}</span>
    {control !== undefined && <span className="m5-setting-control">{control}</span>}
    {children !== undefined && children !== null && children !== false && <div className="m5-setting-detail">{children}</div>}
  </Row>;
}

/** A setting row that opens below; the summary reads as the row. */
export function ExpandableSetting({ title, help, status, children, className = "", defaultOpen = false, openLabel }: {
  title: ReactNode; help?: ReactNode; status?: ReactNode; children: ReactNode; className?: string; defaultOpen?: boolean; openLabel: string;
}) {
  return <details className={`m5-setting m5-setting-expand ${className}`.trim()} open={defaultOpen || undefined}>
    <summary>
      <span className="m5-setting-text"><strong>{title}</strong>{help && <small>{help}</small>}</span>
      {status}
      <span className="m5-setting-open" aria-hidden="true">{openLabel}</span>
    </summary>
    <div className="m5-setting-detail">{children}</div>
  </details>;
}

/** An on/off setting: a native checkbox shown as a switch (keyboard and checks as before). */
export function Switch({ checked, onChange, label, disabled }: { checked: boolean; onChange: (value: boolean) => void; label: string; disabled?: boolean }) {
  return <input type="checkbox" role="switch" className="m5-switch" aria-label={label} checked={checked} disabled={disabled} onChange={event => onChange(event.target.checked)} />;
}
