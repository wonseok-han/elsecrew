/**
 * The result preview's switch (plan 2.5): off unless the app starts with
 * `ELSECREW_PREVIEW=1`, until its independent review closes. Kept apart from
 * `main.ts`, which reads no environment itself; these two names are the only
 * ones read here.
 */
export interface PreviewFlagsV1 { enabled: boolean; devTools: boolean }

export function previewFlagsFromEnvironment(environment: Record<string, string | undefined>, packaged: boolean): PreviewFlagsV1 {
  const enabled = environment.ELSECREW_PREVIEW === "1";
  // The preview's own developer tools: development builds only, and only on request.
  return { enabled, devTools: enabled && !packaged && environment.ELSECREW_PREVIEW_DEVTOOLS === "1" };
}

export function previewFlags(packaged: boolean): PreviewFlagsV1 {
  return previewFlagsFromEnvironment(process.env, packaged);
}
