/**
 * Text that looks like a secret (custom roles R4, risk R7). A role's
 * responsibility goes to the provider on every run, so a pasted key would
 * leave the computer each time. The screen warns before saving; it never
 * blocks, and nothing here is sent anywhere.
 */
const SECRET_SHAPES: readonly RegExp[] = [
  /\bsk-(?:ant-|proj-)?[A-Za-z0-9_-]{16,}/, // OpenAI, Anthropic
  /\bgh[pousr]_[A-Za-z0-9]{20,}/, // GitHub tokens
  /\bgithub_pat_[A-Za-z0-9_]{20,}/,
  /\b(?:AKIA|ASIA)[0-9A-Z]{16}\b/, // AWS access key IDs
  /\bAIza[0-9A-Za-z_-]{30,}/, // Google API keys
  /\bxox[abposr]-[A-Za-z0-9-]{10,}/, // Slack tokens
  /\bglpat-[A-Za-z0-9_-]{20,}/, // GitLab
  /\beyJ[A-Za-z0-9_-]{10,}\.eyJ[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}/, // JWT
  /-----BEGIN [A-Z ]*PRIVATE KEY-----/,
];

export function looksLikeSecret(text: string): boolean {
  return SECRET_SHAPES.some(shape => shape.test(text));
}

/* ---------------------------------------------------------------------------
 * Hiding secrets in a command or its output (work history, 2026-11-26).
 *
 * The same rules run in the engine before a row is stored
 * (`crates/elsecrew-sidecar/src/agent_runtime/preparation/activity.rs`
 * `mask_secrets`) and here before it is drawn, so rows stored by an older
 * build are covered too. Both read `tests/fixtures/m5/secret-mask-cases.json`;
 * a new rule goes into both and gets a line there.
 *
 * Word by word: a known key shape (the list above) keeps its prefix and hides
 * the rest; a value given to a secret-named setting (`GITHUB_TOKEN=…`,
 * `--password …`, `"apiKey": "…"`), an `Authorization` header, a `Bearer`
 * token, a password in a URL and a private key block are hidden whole.
 * ------------------------------------------------------------------------- */
export const SECRET_MASK = "••••";

const SECRET_WORDS = new Set(["token", "secret", "password", "passwd", "pwd", "pass", "passphrase", "apikey", "auth", "credential", "credentials", "cookie", "privatekey", "accesskey", "authorization"]);
const KEY_PARTS = new Set(["api", "access", "private", "secret", "signing", "encryption"]);
const SECRET_ENDINGS = ["token", "secret", "password", "apikey"];

/** A setting's name that holds a secret: GITHUB_TOKEN, --password, apiKey, PGPASSWORD. */
export function secretName(name: string): boolean {
  // camelCase reads as words too (apiKey → api key).
  const words = name.replace(/^-+/, "").replace(/([a-z0-9])([A-Z])/g, "$1_$2").toLowerCase();
  const segments = words.split(/[^a-z0-9]+/).filter(Boolean);
  const flat = segments.join("");
  return segments.some(segment => SECRET_WORDS.has(segment))
    || segments.some((segment, index) => segment === "key" && index > 0 && KEY_PARTS.has(segments[index - 1]!))
    || SECRET_ENDINGS.some(ending => flat.endsWith(ending));
}

const IDENTIFIER = /^[A-Za-z_][A-Za-z0-9_.-]*$/;
const TRAILING = /[,;)}\]]+$/;
const TOKEN_CHAR = /[A-Za-z0-9_-]/;
const ALNUM = /[A-Za-z0-9]/;
const PREFIXES: readonly { prefix: string; min: number; chars: RegExp; exact?: boolean }[] = [
  { prefix: "github_pat_", min: 20, chars: /[A-Za-z0-9_]/ },
  { prefix: "sk-", min: 16, chars: TOKEN_CHAR },
  ...["ghp_", "gho_", "ghu_", "ghs_", "ghr_"].map(prefix => ({ prefix, min: 20, chars: /[A-Za-z0-9]/ })),
  ...["xoxa-", "xoxb-", "xoxp-", "xoxo-", "xoxs-", "xoxr-"].map(prefix => ({ prefix, min: 10, chars: /[A-Za-z0-9-]/ })),
  { prefix: "glpat-", min: 20, chars: TOKEN_CHAR },
  { prefix: "AIza", min: 30, chars: TOKEN_CHAR },
  { prefix: "AKIA", min: 16, chars: /[0-9A-Z]/, exact: true },
  { prefix: "ASIA", min: 16, chars: /[0-9A-Z]/, exact: true },
  { prefix: "eyJ", min: 30, chars: /[A-Za-z0-9_.-]/ },
];

/** Known key shapes inside one word: keep the prefix, hide the rest. */
function maskShapes(word: string): string {
  let out = "";
  let index = 0;
  outer: while (index < word.length) {
    const before = index === 0 ? "" : word[index - 1]!;
    if (!ALNUM.test(before)) {
      for (const { prefix, min, chars, exact } of PREFIXES) {
        if (!word.startsWith(prefix, index)) continue;
        let end = index + prefix.length;
        while (end < word.length && chars.test(word[end]!)) end++;
        const run = word.slice(index + prefix.length, end);
        const shaped = exact ? run.length === min && !ALNUM.test(word[end] ?? "")
          : run.length >= min && (prefix !== "eyJ" || run.split(".").length >= 3);
        if (!shaped) continue;
        out += prefix + SECRET_MASK;
        index = end;
        continue outer;
      }
    }
    out += word[index];
    index++;
  }
  return out;
}

/** A password in a URL: scheme://user:•••• @host. */
function maskUrl(word: string): string {
  const scheme = word.indexOf("://");
  if (scheme < 0) return word;
  const start = scheme + 3;
  const at = word.indexOf("@", start);
  const slash = word.indexOf("/", start);
  if (at < 0 || (slash >= 0 && slash < at)) return word;
  const colon = word.indexOf(":", start);
  if (colon < 0 || colon > at || colon + 1 === at) return word;
  return `${word.slice(0, colon + 1)}${SECRET_MASK}${word.slice(at)}`;
}

type Expect = null | "value" | "long";
interface MaskState { expect: Expect; quote: string | null; line: boolean }

/** A setting's value: quotes and trailing punctuation stay so the line still reads. */
function maskValue(value: string, state: MaskState): string {
  const quote = value[0] === '"' || value[0] === "'" ? value[0] : null;
  if (quote) {
    const close = value.indexOf(quote, 1);
    if (close < 0) { state.quote = quote; return quote + SECRET_MASK; }
    return quote + SECRET_MASK + value.slice(close);
  }
  const suffix = value.match(TRAILING)?.[0] ?? "";
  return SECRET_MASK + suffix;
}

function stripName(text: string): string {
  return text.replace(/^[-{(\["']+/, "").replace(/["']+$/, "");
}

function maskWord(word: string, state: MaskState): string {
  const opening = word.match(/^[{(\["']*/)![0];
  const core = word.slice(opening.length);
  const lower = core.toLowerCase();
  // Authorization: everything after it on the line (up to a closing quote).
  if (lower.startsWith("authorization:") || lower.startsWith("authorization=")) {
    const rest = core.slice("authorization:".length);
    state.line = true;
    if (!rest) return word;
    const quote = rest.match(/["']$/)?.[0] ?? "";
    if (quote) state.line = false;
    return opening + core.slice(0, "authorization:".length) + SECRET_MASK + quote;
  }
  // NAME=value, --name=value
  const equals = word.indexOf("=");
  if (equals > 0) {
    const name = stripName(word.slice(0, equals));
    if (IDENTIFIER.test(name) && secretName(name)) {
      const value = word.slice(equals + 1);
      if (!value) { state.expect = "value"; return word; }
      return word.slice(0, equals + 1) + maskValue(value, state);
    }
  }
  // "name": value, name: value
  const colon = word.indexOf(":");
  if (colon > 0 && !word.startsWith("://", colon)) {
    const name = stripName(word.slice(0, colon));
    if (IDENTIFIER.test(name) && secretName(name)) {
      const value = word.slice(colon + 1);
      if (!value) { state.expect = "value"; return word; }
      return word.slice(0, colon + 1) + maskValue(value, state);
    }
  }
  // --password value
  if (/^-{1,2}[A-Za-z]/.test(word) && IDENTIFIER.test(word.replace(/^-+/, "")) && secretName(word)) {
    state.expect = "value";
    return word;
  }
  if (word === "Bearer" || word === "Basic") { state.expect = "long"; return word; }
  return maskUrl(maskShapes(word));
}

/** Text with the parts that look like secrets replaced by ••••. */
export function maskSecrets(text: string): string {
  // A private key block, whole.
  text = text.replace(/-----BEGIN [A-Z ]*PRIVATE KEY-----[\s\S]*?(?:-----END [A-Z ]*PRIVATE KEY-----|$)/g, `PRIVATE KEY ${SECRET_MASK}`);
  const state: MaskState = { expect: null, quote: null, line: false };
  let out = "";
  for (const piece of text.match(/\s+|\S+/g) ?? []) {
    if (/^\s/.test(piece)) {
      if (piece.includes("\n")) { state.expect = null; state.quote = null; state.line = false; }
      out += piece;
      continue;
    }
    if (state.line) {
      const quote = piece.match(/["']$/)?.[0] ?? "";
      if (quote) state.line = false;
      out += SECRET_MASK + quote;
      continue;
    }
    if (state.quote) {
      const close = piece.indexOf(state.quote);
      if (close < 0) { out += SECRET_MASK; continue; }
      out += SECRET_MASK + piece.slice(close);
      state.quote = null;
      continue;
    }
    if (state.expect) {
      const expect = state.expect;
      state.expect = null;
      if (expect === "value" && !piece.startsWith("-")) { out += maskValue(piece, state); continue; }
      if (expect === "long" && piece.length >= 8) { out += maskValue(piece, state); continue; }
    }
    out += maskWord(piece, state);
  }
  return out;
}
