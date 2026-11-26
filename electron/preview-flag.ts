/**
 * The result preview's switch (plan 2.5). On by default since its isolation
 * probe (I5) and the preview tab's tests passed (2026-11-26); starting the
 * app with `ELSECREW_PREVIEW=0` turns it off (no scheme, no Chromium switch,
 * no view), the way back if a problem shows. Kept apart from `main.ts`,
 * which reads no environment itself; these two names are the only ones read
 * here.
 */
export interface PreviewFlagsV1 { enabled: boolean; devTools: boolean }

export function previewFlagsFromEnvironment(environment: Record<string, string | undefined>, packaged: boolean): PreviewFlagsV1 {
  const enabled = environment.ELSECREW_PREVIEW !== "0";
  // The preview's own developer tools: development builds only, and only on request.
  return { enabled, devTools: enabled && !packaged && environment.ELSECREW_PREVIEW_DEVTOOLS === "1" };
}

export function previewFlags(packaged: boolean): PreviewFlagsV1 {
  return previewFlagsFromEnvironment(process.env, packaged);
}
