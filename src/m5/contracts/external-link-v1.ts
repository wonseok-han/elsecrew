/**
 * Opening a web address from a team report in the person's own browser.
 *
 * The renderer only asks; the main process checks the address again, shows
 * it in a native confirmation the page cannot answer for the person, and only
 * then hands it to the system. Nothing navigates inside the app.
 */
export const EXTERNAL_LINK_CHANNEL = "m5:external-link:v1";
export const EXTERNAL_LINK_MAX = 2048;

/** An absolute http(s) or mailto address without credentials, else null. */
export function validateExternalLink(input: unknown): string | null {
  if (typeof input !== "string" || !input || input.length > EXTERNAL_LINK_MAX || /[\s\p{Cc}\p{Cf}]/u.test(input)) return null;
  let url: URL;
  try { url = new URL(input); } catch { return null; }
  if (url.protocol !== "http:" && url.protocol !== "https:" && url.protocol !== "mailto:") return null;
  if (url.username || url.password) return null;
  if (url.protocol !== "mailto:" && !url.hostname) return null;
  return url.href;
}

/** What the main process answers: whether the person chose to open it. */
export function validateExternalLinkResult(value: unknown): { opened: boolean } {
  if (!value || typeof value !== "object" || Object.keys(value).join() !== "opened" || typeof (value as { opened: unknown }).opened !== "boolean") throw new Error("invalid external link result");
  return { opened: (value as { opened: boolean }).opened };
}
