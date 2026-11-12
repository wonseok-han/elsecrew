import type { ModelChoicesRequest, ModelChoicesResult } from "./contracts/model-choices-v1.ts";
type Runtime = ModelChoicesRequest["runtime"];
// Session cache only: display data is never execution authorization.
const cache = new Map<Runtime, { result: ModelChoicesResult; at: number }>();
const pending = new Map<Runtime, Promise<ModelChoicesResult>>();
let queue: Promise<unknown> = Promise.resolve();
export function catalogCheckedAt(runtime: Runtime) { return cache.get(runtime)?.at; }
export function cachedModels(runtime: Runtime) { return cache.get(runtime)?.result.models; }
export function loadModels(runtime: Runtime, request: (input: ModelChoicesRequest) => Promise<ModelChoicesResult>, refresh = false): Promise<ModelChoicesResult> {
  const existing = pending.get(runtime);
  if (existing) return existing;
  const previous = cache.get(runtime);
  if (!refresh && previous && Date.now() - previous.at < 60_000) return Promise.resolve(previous.result);
  const job = queue.catch(() => undefined).then(() => request({ runtime, refresh: true })).then(result => {
    if (result.runtime !== runtime || result.scope !== null) throw new Error("model catalog scope differs");
    cache.set(runtime, { result, at: Date.now() }); return result;
  }).finally(() => { pending.delete(runtime); });
  pending.set(runtime, job); queue = job;
  return job;
}
