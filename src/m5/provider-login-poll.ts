import type { LoginState } from "./contracts/provider-login-v1.ts";

/** Status reads observe an existing operation; they never refresh credentials. */
export function shouldPollLogin(state: LoginState, transportFailed: boolean, disabled: boolean): boolean {
  return !disabled && (transportFailed || ["diagnosing", "installed", "starting", "waiting", "verifying"].includes(state));
}
