/** A renderer-local start-screen paint opportunity, not whole-app startup time. */
export function markStartScreenReady(): () => void {
  const name = "m5:start-screen-ready";
  if (performance.getEntriesByName(name, "mark").length) return () => {};
  let stopped = false;
  let frame: number | undefined;
  const ready = () => {
    const button = document.querySelector<HTMLButtonElement>(".m5-start-action button");
    return document.visibilityState === "visible" && Boolean(button && !button.disabled && button.getClientRects().length);
  };
  const cancel = () => {
    if (frame !== undefined) cancelAnimationFrame(frame);
    frame = undefined;
  };
  const stop = () => {
    stopped = true;
    cancel();
    document.removeEventListener("visibilitychange", schedule);
  };
  const schedule = () => {
    cancel();
    if (stopped || !ready()) return;
    frame = requestAnimationFrame(() => {
      frame = undefined;
      if (stopped || !ready()) return;
      frame = requestAnimationFrame(() => {
        frame = undefined;
        if (stopped || !ready()) return;
        // Telemetry must never block the UI or grant readiness/permissions.
        try { performance.mark(name); } catch { /* Optional measurement only. */ } finally { stop(); }
      });
    });
  };
  document.addEventListener("visibilitychange", schedule);
  schedule();
  return stop;
}
