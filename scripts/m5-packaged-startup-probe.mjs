// Serialized into the renderer; keep this function self-contained.
export function readPackagedStartupMark() {
  if (document.visibilityState !== 'visible') return {error:'Startup observation window is hidden'};
  const entries = performance.getEntriesByName('m5:start-screen-ready', 'mark');
  if (entries.length === 0) return null; // The product records after two frames.
  if (entries.length !== 1 || !Number.isFinite(entries[0].startTime) || entries[0].startTime < 0
      || !Number.isFinite(performance.timeOrigin) || performance.timeOrigin <= 0) {
    return {error:'Startup readiness mark is invalid or ambiguous'};
  }
  return {timeOrigin:performance.timeOrigin, marks:[entries[0].startTime],
    visibility:document.visibilityState,
    scope:'renderer-local two-frame start-screen readiness; not app spawn duration'};
}
