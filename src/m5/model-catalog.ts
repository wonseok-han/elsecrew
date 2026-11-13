import { validateModelChoicesResult, type ModelChoicesRequest, type ModelChoicesResult } from "./contracts/model-choices-v1.ts";
type Runtime = ModelChoicesRequest["runtime"];
// Display data only: execution still preflights the exact selected model, so a
// remembered list never authorizes a run. It is kept across restarts so the
// role screens show choices immediately instead of re-querying each time.
const FRESH_MS = 6 * 60 * 60 * 1000;
const STORAGE_PREFIX = "elsecrew.m5.models.v1:";
const cache = new Map<Runtime, { result: ModelChoicesResult; at: number }>();
const pending = new Map<Runtime, Promise<ModelChoicesResult>>();
let queue: Promise<unknown> = Promise.resolve();

function storage(): Storage | null {
  try { return typeof localStorage === "undefined" ? null : localStorage; } catch { return null; }
}
function restore(runtime: Runtime) {
  if (cache.has(runtime)) return cache.get(runtime);
  const store = storage();
  if (!store) return undefined;
  try {
    const raw = store.getItem(STORAGE_PREFIX + runtime);
    if (!raw || raw.length > 256 * 1024) return undefined;
    const value = JSON.parse(raw) as { at?: unknown; result?: unknown };
    if (typeof value.at !== "number" || !Number.isFinite(value.at) || value.at > Date.now()) return undefined;
    const result = validateModelChoicesResult(value.result, { runtime, refresh: true });
    if (result.runtime !== runtime || result.scope !== null) return undefined;
    // Usage is a live reading; never replay a remembered one.
    const { usage: _usage, ...models } = result;
    const entry = { result: models as ModelChoicesResult, at: value.at };
    cache.set(runtime, entry);
    return entry;
  } catch { return undefined; }
}
function remember(runtime: Runtime, result: ModelChoicesResult, at: number) {
  cache.set(runtime, { result, at });
  try {
    const { usage: _usage, ...models } = result;
    storage()?.setItem(STORAGE_PREFIX + runtime, JSON.stringify({ at, result: models }));
  } catch { /* the session cache still works */ }
}
export function catalogCheckedAt(runtime: Runtime) { return restore(runtime)?.at; }
export function cachedModels(runtime: Runtime) { return restore(runtime)?.result.models; }
/**
 * A role has a usable model when one is saved and the provider's last known
 * list includes it. Anything else reads as "no model selected"; before any
 * list is known a saved name is trusted (the pre-run check still decides).
 */
export function hasUsableModel(runtime: Runtime, selector: string, models: readonly { modelSelector: string }[] | undefined = cachedModels(runtime)): boolean {
  return Boolean(selector) && (!models?.length || models.some(model => model.modelSelector === selector));
}
/** "opus · claude-opus-5-5" when the provider reported what the alias maps to. */
export function modelLabel(runtime: Runtime, selector: string): string {
  const resolved = restore(runtime)?.result.models.find(model => model.modelSelector === selector)?.resolvedModel;
  return resolved ? `${selector} · ${resolved}` : selector;
}
export function loadModels(runtime: Runtime, request: (input: ModelChoicesRequest) => Promise<ModelChoicesResult>, refresh = false): Promise<ModelChoicesResult> {
  const existing = pending.get(runtime);
  if (existing) return existing;
  const previous = restore(runtime);
  if (!refresh && previous && Date.now() - previous.at < FRESH_MS) return Promise.resolve(previous.result);
  const job = queue.catch(() => undefined).then(() => request({ runtime, refresh: true })).then(result => {
    if (result.runtime !== runtime || result.scope !== null) throw new Error("model catalog scope differs");
    remember(runtime, result, Date.now()); return result;
  }).finally(() => { pending.delete(runtime); });
  pending.set(runtime, job); queue = job;
  return job;
}
