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
