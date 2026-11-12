import { M5_BRIDGE_V4_METHODS, type M5BridgeV4 } from "./contracts/bridge-v4.ts";
import { M5_BRIDGE_V5_METHODS, type M5BridgeV5 } from "./contracts/bridge-v5.ts";
import { M5_BRIDGE_V6_METHODS, type M5BridgeV6 } from "./contracts/bridge-v6.ts";
import { M5_BRIDGE_V7_METHODS, type M5BridgeV7 } from "./contracts/bridge-v7.ts";
import { M5_BRIDGE_V8_METHODS, type M5BridgeV8 } from "./contracts/bridge-v8.ts";

export type SupportedM5Bridge = M5BridgeV4 | M5BridgeV5 | M5BridgeV6 | M5BridgeV7 | M5BridgeV8;

/** Renderer input is untrusted: only a complete, known fixed bridge can reach durable controllers. */
export function resolveM5Bridge(value: unknown): SupportedM5Bridge | null {
  if (typeof value !== "object" || value === null || Array.isArray(value)) return null;
  const candidate = value as Record<string, unknown>;
  const methods = candidate.version === 4
    ? M5_BRIDGE_V4_METHODS
    : candidate.version === 5
      ? M5_BRIDGE_V5_METHODS
      : candidate.version === 6
        ? M5_BRIDGE_V6_METHODS
        : candidate.version === 7 ? M5_BRIDGE_V7_METHODS
          : candidate.version === 8 ? M5_BRIDGE_V8_METHODS : null;
  if (methods === null || methods.some((method) => typeof candidate[method] !== "function")) return null;
  return candidate as unknown as SupportedM5Bridge;
}
