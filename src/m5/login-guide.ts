// Login failures as F11 guides (ADE U5, DESIGN 2.1 rule 5): each kind says what
// happened (the cautious text of provider-login-message.ts), one action to try
// now and the alternatives. The raw error key only goes into the help details.
import type { LoginProvider } from "./contracts/provider-login-v1.ts";

/** What the person can do from the guide. */
export type LoginGuideAction = "retryLogin" | "check" | "install" | "switchRoles";
/** The seven kinds of login failure the app tells apart. */
export type LoginGuideKind = "recovery" | "busy" | "changed" | "missing" | "profile" | "profileCompatibility" | "failed";
export const LOGIN_GUIDE_KINDS: readonly LoginGuideKind[] = ["recovery", "busy", "changed", "missing", "profile", "profileCompatibility", "failed"];

export interface LoginGuide {
  kind: LoginGuideKind;
  /** attention: nothing is lost and it may clear by itself; danger: the agent cannot be used now. */
  tone: "attention" | "danger";
  primary: LoginGuideAction;
  alternatives: readonly LoginGuideAction[];
}

export function loginGuideKind(error: string | null): LoginGuideKind {
  switch (error) {
    case "providerLoginRecoveryRequired": return "recovery";
    case "providerBusy": return "busy";
    case "providerExecutableChanged": return "changed";
    case "providerExecutableMissing": return "missing";
    case "providerProfileUnavailable": return "profile";
    case "providerProfileIsolationRequired": return "profileCompatibility";
    default: return "failed";
  }
}

/**
 * The guide for one failure. Never offers a new sign-in where signing in again
 * could hide the real problem (a stuck login, a changed or missing program, an
 * unreadable or incompatible saved login): those check again, or install.
 */
export function loginGuide(provider: LoginProvider, error: string | null): LoginGuide {
  const kind = loginGuideKind(error);
  switch (kind) {
    case "recovery": return { kind, tone: "attention", primary: "check", alternatives: ["switchRoles"] };
    case "busy": return { kind, tone: "attention", primary: "check", alternatives: [] };
    case "changed": return { kind, tone: "attention", primary: "check", alternatives: ["install", "switchRoles"] };
    case "missing": return { kind, tone: "danger", primary: "install", alternatives: ["check", "switchRoles"] };
    case "profile": return { kind, tone: "danger", primary: "check", alternatives: ["switchRoles"] };
    case "profileCompatibility": return { kind, tone: "danger", primary: "check", alternatives: ["switchRoles"] };
    case "failed": return provider === "claude"
      // An expired or wrong code: a new sign-in gives a new code.
      ? { kind, tone: "danger", primary: "retryLogin", alternatives: ["switchRoles"] }
      // Codex keeps its saved login: check it first; signing in again is the alternative.
      : { kind, tone: "danger", primary: "check", alternatives: ["retryLogin", "switchRoles"] };
  }
}
