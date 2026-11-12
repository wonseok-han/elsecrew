import { getPriority, setPriority } from "node:os";

const maxWorkers = 4;

function cappedWorkers(value) {
  const workers = Number(value);
  return Number.isSafeInteger(workers) && workers > 0
    ? String(Math.min(workers, maxWorkers))
    : String(maxWorkers);
}

export function qaEnvironment(env = process.env) {
  return {
    ...env,
    CI: "1",
    NO_COLOR: "1",
    TZ: "UTC",
    CARGO_BUILD_JOBS: cappedWorkers(env.CARGO_BUILD_JOBS),
    RUST_TEST_THREADS: cappedWorkers(env.RUST_TEST_THREADS),
  };
}

// Lower only this harness process. POSIX children inherit its nice value;
// this is scheduling preference, not a CPU-percentage or global worker limit.
export function lowerQaPriority(get = getPriority, set = setPriority) {
  try {
    const before = get(0);
    const target = Math.max(before, 10);
    if (before < target) set(0, target);
    const actual = get(0);
    return { requested: target, actual, error: actual < target ? "Priority was not lowered" : null };
  } catch (error) {
    return { requested: 10, actual: null, error: String(error) };
  }
}
