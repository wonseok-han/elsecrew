import type { LoginProvider } from "./contracts/provider-login-v1.ts";

export const PROVIDER_SELECTION_KEY = "hello-agent.m5.providers.v1";
export const LOGIN_PROVIDERS: readonly LoginProvider[] = ["codex", "claude"];

// This is a UI preference, never executable admission or authentication proof.
export function parseProviderSelection(raw: string | null): LoginProvider[] {
  if (!raw || raw.length > 128) return [];
  try {
    const value: unknown = JSON.parse(raw);
    if (!value || typeof value !== "object" || Array.isArray(value)) return [];
    const record = value as Record<string, unknown>;
    if (Object.keys(record).sort().join() !== "providers,version" || record.version !== 1 || !Array.isArray(record.providers)) return [];
    const providers = record.providers;
    if (providers.length < 1 || providers.length > 2 || new Set(providers).size !== providers.length || providers.some(p => !LOGIN_PROVIDERS.includes(p))) return [];
    return LOGIN_PROVIDERS.filter(provider => providers.includes(provider));
  } catch { return []; }
}

export function readProviderSelection(storage?: Pick<Storage, "getItem">): LoginProvider[] {
  try { return parseProviderSelection((storage ?? window.localStorage).getItem(PROVIDER_SELECTION_KEY)); }
  catch { return []; }
}

export function saveProviderSelection(providers: readonly LoginProvider[], storage?: Pick<Storage, "setItem">): boolean {
  const raw = JSON.stringify({version: 1, providers});
  if (parseProviderSelection(raw).length !== providers.length || providers.length === 0) return false;
  try { (storage ?? window.localStorage).setItem(PROVIDER_SELECTION_KEY, raw); return true; }
  catch { return false; }
}
