// Fixed codes only: never display exception details, OAuth URLs, or paths.
const diagnostics = [
  "providerLoginCapabilityUnavailable",
  "providerLoginRequestRejected",
  "providerLoginRequestBlocked",
  "providerLoginResponseInvalid",
  "providerLoginTransportUnavailable",
] as const;
export type LoginDiagnostic = typeof diagnostics[number];

export function classifyLoginFailure(error: unknown): LoginDiagnostic {
  const message = error instanceof Error ? error.message : "";
  if (message === "login unavailable") return "providerLoginCapabilityUnavailable";
  if (message.startsWith("invalidParams:")) return "providerLoginRequestRejected";
  if (message.startsWith("requestBlocked:")) return "providerLoginRequestBlocked";
  if (["invalid login status", "invalid attempt", "invalid login error", "invalid login URL"].includes(message)) return "providerLoginResponseInvalid";
  return "providerLoginTransportUnavailable";
}

export function readLoginDiagnostic(error: unknown): LoginDiagnostic {
  const message = error instanceof Error ? error.message : "";
  // Electron prefixes rejected invoke errors. Match the complete fixed suffix.
  return diagnostics.find(code => message === code || message === `Error invoking remote method 'm5:login:v1': Error: ${code}`)
    ?? "providerLoginTransportUnavailable";
}
