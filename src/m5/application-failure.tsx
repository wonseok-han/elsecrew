import { useI18n } from "../i18n.tsx";
import type { ApplicationStatus } from "./contracts/project-catalog-v1.ts";

export function ApplicationFailure({ failure }: { failure: ApplicationStatus["failure"] }) {
  const { t } = useI18n();
  if (!failure) return null;
  const detail = failure.detail.replace(/[\p{Cc}\p{Cf}]/gu, character => `\\u{${character.codePointAt(0)!.toString(16)}}`);
  return <section aria-label={t("m5.application.failureTitle")}>
    <p>{t("m5.application.failureHelp")}</p>
    <details><summary>{t("m5.editor.details")}</summary><code>{failure.code}</code><pre style={{whiteSpace:"pre-wrap",overflowWrap:"anywhere"}}>{detail}</pre></details>
    <p>{t("m5.application.failureSession")}</p>
  </section>;
}
