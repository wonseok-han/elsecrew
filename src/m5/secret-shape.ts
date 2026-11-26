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
 * Hiding secrets in work history text (2026-11-26).
 *
 * The engine masks every row before it is stored
 * (`crates/elsecrew-sidecar/src/agent_runtime/preparation/secret_mask.rs`);
 * this is the same algorithm, run again before a row is drawn. Both read
 * `tests/fixtures/m5/secret-mask-cases.json` and must give the same result;
 * a new rule goes into both and gets a case there. See the Rust module for
 * the list of rules.
 * ------------------------------------------------------------------------- */
export const SECRET_MASK = "••••";
const MASK = SECRET_MASK;

const SECRET_WORDS = new Set(["token", "secret", "password", "passwd", "pwd", "pass", "passphrase", "apikey", "auth", "credential", "credentials", "cookie", "privatekey", "accesskey", "authorization", "sig", "signature"]);
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

const WHITE = /^[ \t\n\r\v\f]/;
const PIECES = /[ \t\n\r\v\f]+|[^ \t\n\r\v\f]+/g;
const IDENTIFIER = /^[A-Za-z_][A-Za-z0-9_.-]*$/;
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

function identifier(name: string): boolean { return IDENTIFIER.test(name); }

/** `%5F` → `_` (ASCII only), so an encoded name still reads as a name. */
function percentDecode(text: string): string {
  return text.replace(/%([0-9A-Fa-f]{2})/g, (whole, hex: string) => { const code = parseInt(hex, 16); return code < 128 ? String.fromCharCode(code) : whole; });
}

/** The name part of `name=`/`name:`: percent-decoded, trailing quotes off,
 * and only the identifier at its end (marks or words stuck in front do not hide it). */
function nameOf(raw: string): string {
  const decoded = percentDecode(raw.replace(/["']+$/, ""));
  return decoded.match(/[A-Za-z0-9_.-]*$/)![0].replace(/^[^A-Za-z_]+/, "");
}

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
        out += prefix + MASK;
        index = end;
        continue outer;
      }
    }
    out += word[index];
    index++;
  }
  return out;
}

/** A long run of letters and digits with no name: 32+, upper, lower and digits. */
function maskBlobs(word: string): string {
  return word.replace(/[A-Za-z0-9+=_-]+/g, run => run.length >= 32 && /[A-Z]/.test(run) && /[a-z]/.test(run) && /[0-9]/.test(run) ? MASK : run);
}

/** A password in a URL, or a long user name standing alone. */
function maskUrl(word: string): string {
  const scheme = word.indexOf("://");
  if (scheme < 0) return word;
  const start = scheme + 3;
  const at = word.indexOf("@", start);
  if (at < 0) return word;
  const slash = word.indexOf("/", start);
  if (slash >= 0 && slash < at) return word;
  const user = word.slice(start, at);
  const colon = user.indexOf(":");
  if (colon >= 0 && colon + 1 < user.length) return `${word.slice(0, start + colon + 1)}${MASK}${word.slice(at)}`;
  if (colon < 0 && user.length >= 16) return `${word.slice(0, start)}${MASK}${word.slice(at)}`;
  return word;
}

type Expect = null | "value" | "long" | "userpass";
interface MaskState { expect: Expect; quote: string | null; line: boolean; mysql: boolean }

/** The closing quote not escaped with a backslash. */
function closeQuote(text: string, quote: string): number {
  for (let index = 0; index < text.length; index++) if (text[index] === quote && (index === 0 || text[index - 1] !== "\\")) return index;
  return -1;
}

function maskValue(value: string, state: MaskState): string {
  const quote = value[0] === '"' || value[0] === "'" ? value[0] : null;
  if (quote) {
    const close = closeQuote(value.slice(1), quote);
    if (close < 0) { state.quote = quote; return quote + MASK; }
    return quote + MASK + value.slice(1 + close);
  }
  const suffix = value.match(/[)}\]]+$/)?.[0] ?? "";
  return MASK + suffix;
}

/** `user:password` → `user:••••`. */
function maskUserpass(value: string): string {
  const colon = value.indexOf(":");
  if (colon < 0 || colon + 1 >= value.length) return value;
  const quote = value[0] === '"' || value[0] === "'" ? value[0] : null;
  const tail = quote && value.length > 1 && value.endsWith(quote) ? quote : "";
  return value.slice(0, colon + 1) + MASK + tail;
}

function longLike(word: string): boolean {
  return word.length >= 16 || (word.length >= 8 && /[0-9]/.test(word));
}

function maskSegment(segment: string, state: MaskState): string {
  for (const flag of ["--user=", "--proxy-user="]) if (segment.startsWith(flag)) return flag + maskUserpass(segment.slice(flag.length));
  const equals = segment.indexOf("=");
  if (equals > 0) {
    const name = nameOf(segment.slice(0, equals));
    if (identifier(name) && secretName(name)) {
      const value = segment.slice(equals + 1);
      if (!value) { state.expect = "value"; return segment; }
      return segment.slice(0, equals + 1) + maskValue(value, state);
    }
  }
  const colon = segment.indexOf(":");
  if (colon > 0 && !segment.startsWith("://", colon)) {
    const name = nameOf(segment.slice(0, colon));
    if (identifier(name) && secretName(name)) {
      const value = segment.slice(colon + 1);
      if (!value) { state.expect = "value"; return segment; }
      return segment.slice(0, colon + 1) + maskValue(value, state);
    }
  }
  if (["-u", "-U", "--user", "--proxy-user"].includes(segment)) { state.expect = "userpass"; return segment; }
  if (segment.length > 2 && segment.startsWith("-u") && !segment.startsWith("--") && segment.includes(":")) return "-u" + maskUserpass(segment.slice(2));
  if (state.mysql && segment.length > 2 && segment.startsWith("-p") && !segment.startsWith("--")) return "-p" + MASK;
  const flag = segment.replace(/^-+/, "");
  if (segment.startsWith("-") && segment.length - flag.length <= 2 && /^[A-Za-z]/.test(flag) && identifier(flag) && secretName(flag)) {
    state.expect = "value";
    return segment;
  }
  const lower = segment.toLowerCase();
  if (lower === "bearer" || lower === "basic") { state.expect = "long"; return segment; }
  return maskBlobs(maskShapes(segment));
}

function maskWord(word: string, state: MaskState): string {
  const opening = word.match(/^[{(\["']*/)![0];
  const core = word.slice(opening.length);
  const lower = core.toLowerCase();
  // Authorization: everything after it on the line (up to a closing quote).
  if (lower.startsWith("authorization:") || lower.startsWith("authorization=")) {
    const head = core.slice(0, "authorization:".length);
    const rest = core.slice("authorization:".length);
    state.line = true;
    if (!rest) return word;
    const quote = rest.match(/["']$/)?.[0] ?? "";
    if (quote) state.line = false;
    return opening + head + MASK + quote;
  }
  let rest = maskUrl(word);
  let out = "";
  for (;;) {
    if (state.quote) {
      const close = closeQuote(rest, state.quote);
      if (close < 0) return out + MASK;
      out += MASK + state.quote;
      rest = rest.slice(close + 1);
      state.quote = null;
      continue;
    }
    if (!rest) return out;
    const found = rest.search(/[;&|,?]/);
    const split = found < 0 ? rest.length : found;
    const segment = rest.slice(0, split);
    if (segment) out += maskSegment(segment, state);
    if (split === rest.length) return out;
    out += rest[split];
    rest = rest.slice(split + 1);
  }
}

/** Text with the parts that look like secrets replaced by ••••. */
export function maskSecrets(text: string): string {
  // A private key block, whole; the replacement is not a BEGIN line, so masking twice changes nothing.
  text = text.replace(/-----BEGIN [A-Z ]*PRIVATE KEY-----[\s\S]*?(?:-----END [A-Z ]*PRIVATE KEY-----|$)/g, `PRIVATE KEY ${MASK}`);
  const lower = text.toLowerCase();
  const state: MaskState = { expect: null, quote: null, line: false, mysql: lower.includes("mysql") || lower.includes("mariadb") };
  let out = "";
  for (const piece of text.match(PIECES) ?? []) {
    if (WHITE.test(piece)) {
      // A line break ends a header or an expected value; a quoted value goes on.
      if (piece.includes("\n")) { state.expect = null; state.line = false; }
      out += piece;
      continue;
    }
    if (state.line && !state.quote) {
      const quote = piece.match(/["']$/)?.[0] ?? "";
      if (quote) state.line = false;
      out += MASK + quote;
      continue;
    }
    if (!state.quote && state.expect) {
      const expect = state.expect;
      state.expect = null;
      if (expect === "value") { out += maskValue(piece, state); continue; }
      if (expect === "userpass") { out += maskUserpass(piece); continue; }
      if (longLike(piece)) { out += maskValue(piece, state); continue; }
    }
    out += maskWord(piece, state);
  }
  return out;
}

/* ---------------------------------------------------------------------------
 * A command line, fail-closed (review PR #11 B2, second round). The same
 * algorithm as `redact_command` in
 * crates/elsecrew-sidecar/src/agent_runtime/preparation/command_redact.rs:
 * keep the program name, subcommands of a few multi-command tools, short
 * flag names, paths inside the workspace, a short allowlist of plain words,
 * small numbers, `$NAME` and shell operators; a URL keeps scheme and host;
 * every other argument becomes ••••. A line that cannot be read safely keeps
 * only its program name, with `argsHidden`.
 * ------------------------------------------------------------------------- */
export interface RedactedCommand { text: string; argsHidden: boolean }

const LONGEST = 2000;
const WITH_SUBCOMMANDS = new Set(["git", "npm", "pnpm", "yarn", "npx", "gh", "cargo", "docker", "kubectl", "go", "pip", "pip3", "brew", "bun", "deno", "make", "rustup", "uv", "poetry", "pnpx"]);
const PLAIN_WORDS = new Set(["origin", "main", "master", "HEAD", "dev", "develop", "all", "true", "false", "yes", "no", "none", "on", "off", "install", "test", "build", "run", "start", "status", "diff", "log", "show", "add", "list", "help", "version", "/dev/null"]);
const VALUE_FLAGS = new Set(["-H", "-u", "-U", "-p", "-b", "-d", "-F", "-E", "--header", "--proxy-header", "--user", "--proxy-user", "--password", "--pass", "--passwd", "--passphrase", "--token", "--key", "--secret", "--auth", "--oauth2-bearer", "--bearer", "--cookie", "--data", "--data-raw", "--data-binary", "--data-urlencode", "--form", "--api-key", "--access-token", "--cert", "--private-key", "--client-secret"]);

const isWhite = (c: string) => c === " " || c === "\t";
const isOperator = (c: string) => c === "|" || c === "&" || c === ";" || c === "<" || c === ">";
const isDigit = (c: string) => c >= "0" && c <= "9";
const allOf = (text: string, ok: RegExp) => text.length > 0 && ok.test(text);
interface Token { text: string; quoted: boolean; op: boolean }

function lex(line: string): Token[] | null {
  const chars = Array.from(line);
  const tokens: Token[] = [];
  let word = "";
  let quoted = false;
  let quote: string | null = null;
  let index = 0;
  const flush = () => { if (word || quoted) tokens.push({ text: word, quoted, op: false }); word = ""; quoted = false; };
  while (index < chars.length) {
    const c = chars[index]!;
    const next = chars[index + 1];
    if (quote) {
      if (c === quote) quote = null;
      else if (quote === '"' && (c === "\\" || c === "`" || (c === "$" && (next === "(" || next === "{")))) return null;
      word += c;
      index++;
      continue;
    }
    if (c === "'" || c === '"') { quote = c; quoted = true; word += c; index++; continue; }
    if ("\\`(){}".includes(c)) return null;
    if (c === "$" && (next === "(" || next === "{")) return null;
    if (isWhite(c)) { flush(); index++; continue; }
    if (isOperator(c)) {
      if (c === "<" && next === "<") return null;
      let op = "";
      if (!quoted && word && Array.from(word).every(isDigit)) { op = word; word = ""; } else flush();
      while (index < chars.length && isOperator(chars[index]!)) { op += chars[index]; index++; }
      if (op.endsWith("&") && op.includes(">")) while (index < chars.length && isDigit(chars[index]!)) { op += chars[index]; index++; }
      tokens.push({ text: op, quoted: false, op: true });
      continue;
    }
    word += c;
    index++;
  }
  if (quote) return null;
  flush();
  return tokens;
}

function assignment(word: string): string | null {
  const equals = word.indexOf("=");
  if (equals < 0) return null;
  const name = word.slice(0, equals);
  return /^[A-Za-z_][A-Za-z0-9_]*$/.test(name) ? name : null;
}

function program(word: string): boolean {
  if (Array.from(word).length > 60 || word.startsWith("-") || word.endsWith("/")) return false;
  return word.split("/").every((part, index) => (index === 0 && part === "" && word.startsWith("/")) || allOf(part, /^[A-Za-z0-9._+-]+$/));
}

function keylike(part: string): boolean {
  return Array.from(part).length >= 20 && /[A-Z]/.test(part) && /[a-z]/.test(part) && /[0-9]/.test(part);
}

function workspacePath(word: string): boolean {
  if (word === ".") return true;
  let body = word.startsWith("./") ? word.slice(2) : word;
  if (body.endsWith("/")) body = body.slice(0, -1);
  if (Array.from(word).length > 120 || !body || word.startsWith("/") || word.startsWith("~") || word.startsWith("-")) return false;
  const parts = body.split("/");
  const last = parts[parts.length - 1]!;
  const dot = last.lastIndexOf(".");
  const extension = dot > 0 && last.length - dot - 1 >= 1 && last.length - dot - 1 <= 8 && /^[A-Za-z0-9]+$/.test(last.slice(dot + 1));
  return (parts.length > 1 || extension || word.endsWith("/"))
    && parts.every(part => allOf(part, /^[A-Za-z0-9._@+-]+$/) && part !== ".." && Array.from(part).length <= 64 && !keylike(part));
}

function url(word: string): string | null {
  const at = word.indexOf("://");
  if (at < 0) return null;
  const scheme = word.slice(0, at);
  const rest = word.slice(at + 3);
  if (!allOf(scheme, /^[A-Za-z0-9+.-]+$/)) return null;
  const found = rest.search(/[/?#]/);
  const end = found < 0 ? rest.length : found;
  const authority = rest.slice(0, end);
  const host = authority.slice(authority.lastIndexOf("@") + 1);
  if (!allOf(host, /^[A-Za-z0-9.:-]+$/)) return null;
  return `${scheme}://${host}${rest.length > host.length ? `/${MASK}` : ""}`;
}

const shortFlag = (word: string) => word.length >= 2 && word.length <= 5 && /^-[A-Za-z]+$/.test(word);
const longFlag = (name: string) => name.length > 2 && name.length <= 32 && /^--[a-z0-9][a-z0-9-]*$/.test(name);
const valueFlag = (flag: string) => VALUE_FLAGS.has(flag) || secretName(flag);

function firstProgram(line: string): string {
  const word = line.split(/[ \t\n\r|&;<>]+/).filter(Boolean).find(part => assignment(part) === null);
  return word && program(word) ? word : "";
}

export function redactCommand(input: string): RedactedCommand {
  const line = input.replace(/^[ \t\n\r]+|[ \t\n\r]+$/g, "");
  const hidden = (): RedactedCommand => ({ text: firstProgram(line), argsHidden: true });
  if (!line) return { text: "", argsHidden: false };
  if (Array.from(line).length > LONGEST || line.includes("\n") || line.includes("\r")) return hidden();
  const tokens = lex(line);
  if (!tokens) return hidden();
  const out: string[] = [];
  let expectProgram = true;
  let redirect = false;
  let secretNext = false;
  let subcommands = 0;
  for (const token of tokens) {
    const word = token.text;
    if (token.op) {
      out.push(word);
      if (["|", "||", "&&", ";", "&", "|&", ";;"].includes(word)) { expectProgram = true; secretNext = false; subcommands = 0; }
      else if ((word.includes("<") || word.includes(">")) && !isDigit(word[word.length - 1]!)) redirect = true;
      continue;
    }
    const wasSecret: boolean = secretNext, wasRedirect: boolean = redirect, wasProgram: boolean = expectProgram;
    let shown: string;
    if (token.quoted) {
      const equals = word.indexOf("=");
      const name = equals >= 0 && !/['"]/.test(word.slice(0, equals)) ? word.slice(0, equals) : null;
      if (name !== null && expectProgram && assignment(word) !== null) shown = `${name}=${MASK}`;
      else if (name !== null && !expectProgram && longFlag(name)) shown = `${name}=${MASK}`;
      else { expectProgram = false; shown = MASK; }
    } else if (redirect) {
      shown = workspacePath(word) || PLAIN_WORDS.has(word) ? word : MASK;
    } else if (secretNext) {
      shown = MASK;
    } else if (expectProgram) {
      const name = assignment(word);
      if (name !== null) shown = `${name}=${MASK}`;
      else if (program(word)) {
        subcommands = WITH_SUBCOMMANDS.has(word.slice(word.lastIndexOf("/") + 1)) ? 2 : 0;
        expectProgram = false;
        shown = word;
      } else { expectProgram = false; shown = MASK; }
    } else if (word.startsWith("-")) {
      const equals = word.indexOf("=");
      if (word === "--") shown = word;
      else if (equals >= 0) shown = longFlag(word.slice(0, equals)) ? `${word.slice(0, equals)}=${MASK}` : MASK;
      else if (longFlag(word) || shortFlag(word)) shown = word;
      else if (word.length > 2 && /[A-Za-z]/.test(word[1]!)) shown = word.slice(0, 2) + MASK;
      else shown = MASK;
    } else {
      const asUrl = url(word);
      if (asUrl !== null) shown = asUrl;
      else if (subcommands > 0 && /^[a-z:-]+$/.test(word) && word.length <= 24) { subcommands--; shown = word; }
      else if (workspacePath(word) || PLAIN_WORDS.has(word) || /^[0-9]{1,4}$/.test(word)
        || (word.length <= 40 && /^\$[A-Za-z_][A-Za-z0-9_]*$/.test(word))) shown = word;
      else shown = MASK;
    }
    // A value flag (shown as itself) guards exactly the one word after it.
    secretNext = !token.quoted && !wasSecret && !wasRedirect && !wasProgram && (longFlag(word) || shortFlag(word)) && valueFlag(word);
    redirect = false;
    out.push(shown);
  }
  return { text: out.join(" "), argsHidden: false };
}
