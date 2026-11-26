/**
 * The result preview's switch (plan 2.5). On by default on macOS, the
 * platform its isolation probe and screen smoke run on; off by default on
 * Windows and elsewhere until they are checked there (PR #9 review I2/I5).
 * `ELSECREW_PREVIEW=0` turns it off and `=1` on, on any platform (no scheme,
 * no Chromium switch, no view when off). Kept apart from `main.ts`, which
 * reads no environment itself; these two names are the only ones read here.
 */
export interface PreviewFlagsV1 { enabled: boolean; devTools: boolean }

export const PREVIEW_DEFAULT_PLATFORMS: readonly string[] = Object.freeze(["darwin"]);

export function previewFlagsFromEnvironment(environment: Record<string, string | undefined>, packaged: boolean, platform: string): PreviewFlagsV1 {
  const setting = environment.ELSECREW_PREVIEW;
  const enabled = setting === "1" ? true : setting === "0" ? false : PREVIEW_DEFAULT_PLATFORMS.includes(platform);
  // The preview's own developer tools: development builds only, and only on request.
  return { enabled, devTools: enabled && !packaged && environment.ELSECREW_PREVIEW_DEVTOOLS === "1" };
}

export function previewFlags(packaged: boolean, platform: string): PreviewFlagsV1 {
  return previewFlagsFromEnvironment(process.env, packaged, platform);
}
