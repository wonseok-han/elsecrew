// Login failures as F11 guides (ADE U5, DESIGN 2.1 rule 5): each kind says what
// happened (the cautious text of provider-login-message.ts), one action to try
// now and the alternatives. The raw error key only goes into the help details.
import type { LoginProvider } from "./contracts/provider-login-v1.ts";

/** What the person can do from the guide. */
export type LoginGuideAction = "retryLogin" | "check" | "install" | "switchRoles";
/** The kinds of login failure the app tells apart: the seven the agent reports, and the app not reaching its own check. */
export type LoginGuideKind = "recovery" | "busy" | "changed" | "missing" | "profile" | "profileCompatibility" | "failed" | "unreachable";
export const LOGIN_GUIDE_KINDS: readonly LoginGuideKind[] = ["recovery", "busy", "changed", "missing", "profile", "profileCompatibility", "failed", "unreachable"];

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
    // The request never reached a sign-in (blocked, rejected, no answer): nothing says the login is gone (PR #14 review I1).
    default: return error !== null && /^providerLogin(Capability|Request|Response|Transport)/.test(error) ? "unreachable" : "failed";
  }
}

/**
 * The guide for one failure. Never offers a new sign-in where signing in again
 * could hide the real problem (a stuck login, a changed or missing program, an
 * unreadable or incompatible saved login, the app not reaching its check):
 * those check again, or install. A general failure is not proof that a code
 * expired (it also covers time-outs and unverifiable answers), so checking
 * comes first and a new sign-in is the alternative (PR #14 review I1).
 */
// The same guide for both agents today; the provider stays in the signature for wording that may differ.
export function loginGuide(_provider: LoginProvider, error: string | null): LoginGuide {
  const kind = loginGuideKind(error);
  switch (kind) {
    case "recovery": return { kind, tone: "attention", primary: "check", alternatives: ["switchRoles"] };
    case "busy": return { kind, tone: "attention", primary: "check", alternatives: [] };
    case "changed": return { kind, tone: "attention", primary: "check", alternatives: ["install", "switchRoles"] };
    case "missing": return { kind, tone: "danger", primary: "install", alternatives: ["check", "switchRoles"] };
    case "profile": return { kind, tone: "danger", primary: "check", alternatives: ["switchRoles"] };
    case "profileCompatibility": return { kind, tone: "danger", primary: "check", alternatives: ["switchRoles"] };
    case "unreachable": return { kind, tone: "attention", primary: "check", alternatives: [] };
    case "failed": return { kind, tone: "danger", primary: "check", alternatives: ["retryLogin", "switchRoles"] };
  }
}
