import test from "node:test";
import assert from "node:assert/strict";
import { AppUpdateController, type NativeUpdater } from "../electron/app-update.ts";
import { QuitCoordinatorV1 } from "../electron/quit-coordinator.ts";
function fixture(safe = true) {
  const listeners = new Map<string, (...args: unknown[]) => void>();
  let checks = 0, installs = 0, feeds = 0;
  const native: NativeUpdater = {
    on: (event, listener) => listeners.set(event, listener),
    setFeedURL: () => { feeds++; }, checkForUpdates: () => { checks++; }, quitAndInstall: () => { installs++; },
  };
  const controller = new AppUpdateController(native, async install => {
    const quit = new QuitCoordinatorV1({ prepareSidecarQuit: async () => ({ safeToQuit: safe, blockers: safe ? [] : ["recovery"] }),
      failedBeforeReady: () => false, allowProcessExit: kind => { if (kind === "clean") install(); } });
    return (await quit.prepareQuit()).safeToQuit;
  }, () => {});
  return { controller, emit: (event: string) => listeners.get(event)?.(), counts: () => ({ checks, installs, feeds }) };
}
test("unconfigured, unsigned and unsafe feeds never download", () => {
  for (const [feed, signed] of [[null, true], ["https://updates.example.test", false], ["http://example.test", true], ["https://user:secret@example.test", true]] as const) {
    const f = fixture(); f.controller.configure(feed, signed); f.controller.check();
    assert.equal(f.controller.state, "unavailable"); assert.deepEqual(f.counts(), { checks: 0, installs: 0, feeds: 0 });
    f.emit("update-downloaded"); assert.equal(f.controller.state, "unavailable");
  }
});
test("check deduplicates and download never installs without explicit restart", async () => {
  const f = fixture(); f.controller.configure("https://updates.example.test", true);
  f.controller.check(); f.controller.check(); f.emit("update-available");
  assert.equal(f.controller.state, "downloading"); f.emit("update-downloaded");
  assert.equal(f.counts().installs, 0); await f.controller.restart();
  assert.deepEqual(f.counts(), { checks: 1, installs: 1, feeds: 1 });
});
test("recovery blocker preserves downloaded update without installation", async () => {
  const f = fixture(false); f.controller.configure("https://updates.example.test", true);
  f.controller.check(); f.emit("update-downloaded"); await f.controller.restart();
  assert.equal(f.controller.state, "ready"); assert.equal(f.counts().installs, 0);
});
test("failed shutdown never authorizes installation", async () => {
  let installs = 0;
  const listeners = new Map<string, (...args: unknown[]) => void>();
  const c = new AppUpdateController({ on: (e,l) => listeners.set(e,l), setFeedURL() {}, checkForUpdates() {}, quitAndInstall() { installs++; } },
    async () => { throw Error("disconnected"); }, () => {});
  c.configure("https://updates.example.test", true); c.check(); listeners.get("update-downloaded")?.(); await c.restart();
  assert.equal(c.state, "ready"); assert.equal(installs, 0);
});
test("errors allow retry and no-update reports current", () => {
  const f = fixture(); f.controller.configure("https://updates.example.test", true); f.controller.check(); f.emit("error");
  assert.equal(f.controller.state, "error"); f.controller.check(); f.emit("update-not-available"); assert.equal(f.controller.state, "current");
});
