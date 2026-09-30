import type { LoginProvider } from "./contracts/provider-login-v1.ts";
import { ko } from "../locales/ko.ts";
import { en } from "../locales/en.ts";

export function loginErrorMessage(provider: LoginProvider, error: string | null, lang: "ko" | "en" = "ko"): string {
  const messages = lang === "en" ? en : ko;
  if (error === "providerLoginRecoveryRequired") return messages["m5.login.error.recovery"];
  if (error === "providerExecutableChanged") return messages["m5.login.error.changed"];
  if (error === "providerExecutableMissing") return messages["m5.login.error.missing"];
  if (error === "providerProfileUnavailable") return messages["m5.login.error.profile"];
  return messages[provider === "claude" ? "m5.login.error.claude" : "m5.login.error.codex"];
}
