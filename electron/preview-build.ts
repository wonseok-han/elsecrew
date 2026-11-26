/**
 * Build-then-preview runner (결과 미리보기 확장 1단계, 2026-11-27; PR #22
 * review B1–B4, I2).
 *
 * The person asks to install a task's packages or build it; main asks the
 * sidecar what that task's workspace needs (`preview.buildPlan`, read-only,
 * from the chat binding), shows the exact command, scripts, packages,
 * program and folder in its own native box, and only on "예" runs it. The
 * page can neither name the folder nor the command nor answer the box.
 *
 * - Bound to what was agreed (B1): the plan carries a fingerprint of the
 *   folder and every input file; right before starting main asks again and
 *   runs only if the fingerprint, folder and tools are the ones shown.
 *   Otherwise nothing runs and the box asks again with what is there now.
 * - Confined (B2): the job runs in a macOS sandbox profile — writes only to
 *   the workspace, temporary folders and package caches, no reading of the
 *   app's data or key folders, network only while installing.
 * - Ended (B3): a start waiting for its plan or the box is called off by
 *   cancel, a page reload, the window closing and quit preparation. A
 *   running job is the leader of its own process group; its descendants
 *   are looked for every second (also those that leave the group) and are
 *   ended with it. Any still alive after the end count as leftovers: the
 *   person is told, that task cannot build until they are gone, and the
 *   next app start ends those it can still prove are the same processes.
 * - Masked (B4): each stream's lines go through a stateful masker (PEM
 *   blocks, nameless key material, over-long lines hidden whole) before they
 *   are kept, shown or written; nothing raw reaches memory, IPC or disk.
 * - A build counts as done only when it wrote a new `index.html` into its
 *   output folder and nothing it wrote would be recorded by Git (I2·I1).
 */
import type { ChildProcess, SpawnOptions } from "node:child_process";
import {
  PREVIEW_BUILD_LOG_MAX_LINES_V1,
  type PreviewBuildIpcRequestV1,
  type PreviewBuildIpcResultV1,
  type PreviewBuildJobViewV1,
  type PreviewBuildPlanResultV1,
  type PreviewBuildPlanViewV1,
  type PreviewBuildStartOutcomeV1,
  type PreviewBuildStateV1,
  type PreviewBuildStepV1,
} from "../src/m5/contracts/preview-v1.ts";
import {
  PREVIEW_BUILD_KILL_GRACE_MS,
  PREVIEW_BUILD_MAX_RUNNING,
  PREVIEW_BUILD_PIPE_GRACE_MS,
  PREVIEW_BUILD_TIMEOUT_MS,
  PREVIEW_BUILD_TRACK_MS,
  PREVIEW_INSTALL_TIMEOUT_MS,
  PreviewLineSplitter,
  PreviewLogMasker,
  previewBuildArgs,
  previewBuildCommandLine,
  previewBuildEnvironment,
  previewBuildLogName,
  type PreviewBuildConsentV1,
} from "./preview-build-policy.ts";

export interface PreviewBuildSidecarV1 {
  previewBuildPlan(input: { projectId: string; chatId: string }): Promise<PreviewBuildPlanResultV1>;
}

/** A process seen as part of a job: its id and when it started (so a reused
 * id is never taken for it). */
export interface PreviewProcessV1 { pid: number; ppid: number; pgid: number; started: string }
/** A job's processes kept on disk while it runs, for the next app start. */
export interface PreviewRunningRecordV1 { leader: number; processes: { pid: number; started: string }[] }

export interface PreviewBuildToolsV1 {
  program: string;
  pathDirs: string[];
  /** The program's and `node`'s real files (path, device, inode, size, change time). */
  identity: string;
}

export interface PreviewBuildOptionsV1 {
  enabled: boolean;
  sidecar(): PreviewBuildSidecarV1 | undefined;
  /** main's native box; true only for the person's "예". */
  confirm(consent: PreviewBuildConsentV1): Promise<boolean>;
  /** `node` and the package manager, or null when not found on this computer. */
  findTools(manager: "npm" | "pnpm" | "yarn"): PreviewBuildToolsV1 | null;
  /** The folder is a real directory, reached without links. */
  isWorkspaceFolder(path: string): boolean;
  /** The sandboxed command for a step, or null when it cannot be confined. */
  confine(step: PreviewBuildStepV1, workspace: string, program: string, args: string[]): { program: string; args: string[] } | null;
  spawn(program: string, args: string[], options: SpawnOptions): ChildProcess;
  /** Signals a process group (POSIX `kill(-pgid)`). */
  killGroup(pid: number, signal: NodeJS.Signals): void;
  /** Signals one process, only if it is still the one that started then. */
  killProcess(pid: number, started: string, signal: NodeJS.Signals): void;
  /** Every process of this user now (`ps`). */
  processes(): PreviewProcessV1[];
  /** The process with this id is still the one that started then. */
  isAlive(pid: number, started: string): boolean;
  /** Running jobs' processes on disk (best effort). */
  saveRunning(records: PreviewRunningRecordV1[]): void;
  loadRunning(): PreviewRunningRecordV1[];
  /** `<workspace>/<output>/index.html` is a regular file (no links on the way) changed at or after `since`. */
  outputWritten(workspace: string, output: string, since: number): boolean;
  environment: Readonly<Record<string, string | undefined>>;
  /** Where ended jobs' masked logs are written (best effort). */
  writeLog(name: string, text: string): void;
  log(label: string): void;
  now?(): number;
  timeoutMs?: { install: number; build: number };
  killGraceMs?: number;
  pipeGraceMs?: number;
  trackMs?: number;
}

interface Job {
  readonly key: string;
  readonly projectId: string;
  readonly chatId: string;
  step: PreviewBuildStepV1 | null;
  state: PreviewBuildStateV1;
  /** Lines kept (masked); `base` is the cursor of the first one. */
  lines: string[];
  base: number;
  exitCode: number | null;
  startedAt: number | null;
  endedAt: number | null;
  child: ChildProcess | undefined;
  pid: number | undefined;
  /** Every process seen as part of this job (the leader included). */
  seen: Map<number, string>;
  leaderExited: boolean;
  timers: ReturnType<typeof setTimeout>[];
  tracker: ReturnType<typeof setInterval> | undefined;
  /** Set by a build that succeeded, cleared when another step starts. */
  output: { dir: string; workspaceIdentity: string } | null;
  /** The person asked to stop (or the time ran out) before it ended. */
  stopping: "cancelled" | "timedOut" | null;
  leftovers: number;
}

/** A start waiting for its plan or the box. */
interface PendingStart { aborted: boolean }

const IDLE_VIEW: PreviewBuildJobViewV1 = { step: null, state: "idle", cursor: 0, lines: [], exitCode: null, outputReady: false, startedAt: null, endedAt: null, leftovers: 0 };

export class PreviewBuildManagerV1 {
  readonly #options: PreviewBuildOptionsV1;
  readonly #jobs = new Map<string, Job>();
  readonly #pending = new Map<string, PendingStart>();
  #quitting = false;
  /** Quit preparation in progress: no new start until it is called off. */
  #suspended = false;

  constructor(options: PreviewBuildOptionsV1) {
    this.#options = options;
  }

  /** For tests and probes (never IPC): the live leaders' ids. */
  get runningPids(): number[] {
    return [...this.#jobs.values()].filter(job => job.state === "running" && job.pid !== undefined).map(job => job.pid!);
  }
  /** For probes (never IPC): every process a running job has shown. */
  seenProcesses(projectId: string, chatId: string): number[] {
    return [...(this.#jobs.get(`${projectId}\u0000${chatId}`)?.seen.keys() ?? [])];
  }

  /** The app starts: processes a job left when the app ended without
   * stopping them are ended, if they are still the same processes. */
  recoverLeftovers(): number {
    let ended = 0;
    let records: PreviewRunningRecordV1[] = [];
    try { records = this.#options.loadRunning(); } catch { records = []; }
    for (const record of records) {
      for (const process of record.processes) {
        if (!this.#options.isAlive(process.pid, process.started)) continue;
        try { this.#options.killProcess(process.pid, process.started, "SIGKILL"); ended++; } catch { /* gone */ }
      }
    }
    try { this.#options.saveRunning([]); } catch { /* best effort */ }
    if (ended) this.#options.log(`preview build leftovers ended: ${ended}`);
    return ended;
  }

  async request(request: PreviewBuildIpcRequestV1): Promise<PreviewBuildIpcResultV1> {
    if (!this.#options.enabled) throw new Error("preview build is not enabled");
    if (this.#quitting) throw new Error("preview build is closed while the app quits");
    const key = `${request.projectId}\u0000${request.chatId}`;
    switch (request.action) {
      case "plan": return { action: "plan", plan: await this.#planView(request.projectId, request.chatId), job: this.#view(key, Number.MAX_SAFE_INTEGER) };
      case "status": return { action: "status", job: this.#view(key, request.cursor) };
      case "cancel": {
        const pending = this.#pending.get(key);
        if (pending) pending.aborted = true;
        const job = this.#jobs.get(key);
        if (job) this.#stop(job, "cancelled");
        return { action: "cancel", job: this.#view(key, Number.MAX_SAFE_INTEGER) };
      }
      case "start": return this.#start(key, request.projectId, request.chatId, request.step);
    }
  }

  /** The finished build of this task, for the preview (`source: "build"`). */
  buildOutput(projectId: string, chatId: string): { dir: string; workspaceIdentity: string } | null {
    const job = this.#jobs.get(`${projectId}\u0000${chatId}`);
    return job?.state === "succeeded" && job.step === "build" ? job.output : null;
  }

  /** The app page reloaded or its process ended, the window closed: nobody
   * watches the jobs any more, and a box still open is not the person's yes. */
  stopAll(): void {
    for (const pending of this.#pending.values()) pending.aborted = true;
    for (const job of this.#jobs.values()) this.#stop(job, "cancelled");
  }

  /** Quit preparation (an update, say): everything stops and nothing new
   * starts until it is called off (`resume`). */
  suspend(): void {
    this.#suspended = true;
    this.stopAll();
  }
  resume(): void { if (!this.#quitting) this.#suspended = false; }

  /** Quitting: every job's group and seen process is killed now. */
  shutdown(): void {
    this.#quitting = true;
    for (const pending of this.#pending.values()) pending.aborted = true;
    for (const job of this.#jobs.values()) {
      if (job.state !== "running") continue;
      job.stopping ??= "cancelled";
      this.#kill(job, "SIGKILL");
    }
  }

  async #plan(projectId: string, chatId: string): Promise<PreviewBuildPlanResultV1 | null> {
    const sidecar = this.#options.sidecar();
    if (!sidecar) return null;
    try { return await sidecar.previewBuildPlan({ projectId, chatId }); } catch { return null; }
  }

  async #planView(projectId: string, chatId: string): Promise<PreviewBuildPlanViewV1> {
    const plan = await this.#plan(projectId, chatId);
    if (!plan) return { refusal: "unavailable", installCommand: null, buildCommand: null, needsInstall: false, outputDir: null };
    if (plan.refusal !== null || !plan.packageManager) return { refusal: plan.refusal ?? "unavailable", installCommand: null, buildCommand: null, needsInstall: false, outputDir: null };
    const tools = this.#options.findTools(plan.packageManager);
    return {
      refusal: tools ? null : "toolMissing",
      installCommand: previewBuildCommandLine(plan.packageManager, "install", plan.lockfile, plan.yarnBerry),
      buildCommand: previewBuildCommandLine(plan.packageManager, "build", plan.lockfile, plan.yarnBerry),
      needsInstall: plan.needsInstall,
      outputDir: plan.outputDir,
    };
  }

  #consent(step: PreviewBuildStepV1, plan: PreviewBuildPlanResultV1, tools: PreviewBuildToolsV1, changed: boolean): PreviewBuildConsentV1 {
    return {
      step, changed, command: previewBuildCommandLine(plan.packageManager!, step, plan.lockfile, plan.yarnBerry), program: tools.program,
      folder: plan.workspacePath!, buildScript: step === "build" ? plan.buildScript : null,
      hooks: step === "build" ? plan.buildHooks ?? [] : [],
      dependencies: plan.dependencies ?? [], dependencyCount: plan.dependencyCount ?? 0,
    };
  }

  /** What the person saw is still what would run. */
  #same(a: { plan: PreviewBuildPlanResultV1; tools: PreviewBuildToolsV1 }, b: { plan: PreviewBuildPlanResultV1; tools: PreviewBuildToolsV1 }): boolean {
    return a.plan.fingerprint === b.plan.fingerprint && a.plan.workspacePath === b.plan.workspacePath
      && a.plan.workspaceIdentity === b.plan.workspaceIdentity && a.plan.worktreeId === b.plan.worktreeId
      && a.plan.needsInstall === b.plan.needsInstall && a.tools.identity === b.tools.identity && a.tools.program === b.tools.program;
  }

  async #start(key: string, projectId: string, chatId: string, step: PreviewBuildStepV1): Promise<PreviewBuildIpcResultV1> {
    const answer = (outcome: PreviewBuildStartOutcomeV1, cursor = Number.MAX_SAFE_INTEGER) => ({ action: "start" as const, outcome, job: this.#view(key, cursor) });
    if (this.#suspended) return answer("refused");
    const busy = () => this.#pending.has(key) || this.#jobs.get(key)?.state === "running" || this.#running() >= PREVIEW_BUILD_MAX_RUNNING;
    if (busy()) return answer("busy");
    // An earlier job's processes still alive: this task waits for them.
    const earlier = this.#jobs.get(key);
    if (earlier && earlier.leftovers > 0) {
      earlier.leftovers = [...earlier.seen].filter(([pid, started]) => this.#options.isAlive(pid, started)).length;
      if (earlier.leftovers > 0) return answer("leftovers");
    }
    const pending: PendingStart = { aborted: false };
    this.#pending.set(key, pending);
    try {
      const read = async () => {
        const plan = await this.#plan(projectId, chatId);
        if (!plan || plan.refusal !== null || !plan.packageManager || !plan.workspacePath || !plan.outputDir || !plan.fingerprint) return null;
        // Building needs the packages first (the page offers installing).
        if (step === "build" && plan.needsInstall) return null;
        const tools = this.#options.findTools(plan.packageManager);
        if (!tools || !this.#options.isWorkspaceFolder(plan.workspacePath)) return null;
        return { plan, tools };
      };
      let shown = await read();
      if (pending.aborted) return answer("cancelled");
      if (!shown) return answer("refused");
      if (!(await this.#options.confirm(this.#consent(step, shown.plan, shown.tools, false)))) return answer(pending.aborted ? "cancelled" : "declined");
      // Right before starting: the same folder, inputs and tools as shown
      // (B1). What changed is shown again; nothing runs on the old yes.
      for (let asked = 1; ; asked++) {
        if (pending.aborted || this.#suspended || this.#quitting) return answer("cancelled");
        const now = await read();
        if (pending.aborted || this.#suspended || this.#quitting) return answer("cancelled");
        if (!now) return answer("refused");
        if (this.#same(shown, now)) { shown = now; break; }
        if (asked >= 3) return answer("changed");
        shown = now;
        if (!(await this.#options.confirm(this.#consent(step, shown.plan, shown.tools, true)))) return answer(pending.aborted ? "cancelled" : "changed");
      }
      if (this.#jobs.get(key)?.state === "running" || this.#running() >= PREVIEW_BUILD_MAX_RUNNING) return answer("busy");
      const args = previewBuildArgs(shown.plan.packageManager!, step, shown.plan.lockfile, shown.plan.yarnBerry);
      const confined = this.#options.confine(step, shown.plan.workspacePath!, shown.tools.program, args);
      if (!confined) return answer("refused");
      const base = this.#run(key, projectId, chatId, step, shown.tools, args, confined, shown.plan);
      return answer("started", base);
    } finally {
      if (this.#pending.get(key) === pending) this.#pending.delete(key);
    }
  }

  #running(): number { return [...this.#jobs.values()].filter(job => job.state === "running").length; }

  /** Starts the job; its first cursor (the command line). */
  #run(key: string, projectId: string, chatId: string, step: PreviewBuildStepV1, tools: PreviewBuildToolsV1, args: string[], confined: { program: string; args: string[] }, plan: PreviewBuildPlanResultV1): number {
    const now = this.#options.now ?? Date.now;
    const previous = this.#jobs.get(key);
    const job: Job = {
      key, projectId, chatId, step, state: "running", lines: [], base: (previous?.base ?? 0) + (previous?.lines.length ?? 0),
      exitCode: null, startedAt: now(), endedAt: null, child: undefined, pid: undefined, seen: new Map(), leaderExited: false,
      timers: [], tracker: undefined, output: null, stopping: null, leftovers: 0,
    };
    this.#jobs.set(key, job);
    this.#append(job, `$ ${[plan.packageManager, ...args].join(" ")}`);
    let child: ChildProcess;
    try {
      child = this.#options.spawn(confined.program, confined.args, {
        cwd: plan.workspacePath!,
        env: previewBuildEnvironment(this.#options.environment, tools.pathDirs),
        // Its own session and process group: ending it ends what it started.
        detached: process.platform !== "win32",
        stdio: ["ignore", "pipe", "pipe"],
        shell: false,
        windowsHide: true,
      });
    } catch {
      this.#append(job, "could not start the build program");
      this.#finish(job, null);
      return job.base;
    }
    job.child = child;
    job.pid = child.pid;
    this.#options.log(`preview ${step} started`);
    this.#track(job);
    job.tracker = setInterval(() => this.#track(job), this.#options.trackMs ?? PREVIEW_BUILD_TRACK_MS);
    job.tracker.unref?.();
    const streams = [child.stdout, child.stderr].map(stream => ({ stream, split: new PreviewLineSplitter(), mask: new PreviewLogMasker() }));
    for (const { stream, split, mask } of streams) {
      stream?.setEncoding("utf8");
      stream?.on("data", (chunk: string) => { if (job.state === "running") for (const line of split.push(chunk)) this.#append(job, mask.line(line)); });
    }
    const flush = () => { for (const { split, mask } of streams) for (const line of split.flush()) this.#append(job, mask.line(line)); };
    child.on("error", () => {
      if (job.state !== "running" || job.leaderExited) return;
      job.leaderExited = true;
      this.#append(job, "could not start the build program");
      this.#finish(job, null);
    });
    let code: number | null = null;
    let closed = false;
    let settled = false;
    const settle = () => {
      if (job.state !== "running" || settled) return;
      settled = true;
      flush();
      // Whatever the leader left behind ends with it.
      this.#track(job);
      this.#kill(job, "SIGKILL");
      this.#later(job, 300, () => this.#done(job, code, plan));
    };
    child.on("exit", (exitCode: number | null) => {
      job.leaderExited = true;
      code = exitCode;
      // A descendant may keep the pipes open: the job ends without them.
      this.#later(job, this.#options.pipeGraceMs ?? PREVIEW_BUILD_PIPE_GRACE_MS, () => {
        if (closed) return;
        for (const { stream } of streams) stream?.destroy();
        settle();
      });
    });
    child.on("close", (exitCode: number | null) => {
      closed = true;
      job.leaderExited = true;
      code ??= exitCode;
      settle();
    });
    const limits = this.#options.timeoutMs ?? { install: PREVIEW_INSTALL_TIMEOUT_MS, build: PREVIEW_BUILD_TIMEOUT_MS };
    this.#later(job, step === "install" ? limits.install : limits.build, () => this.#stop(job, "timedOut"));
    return job.base;
  }

  #later(job: Job, ms: number, run: () => void): void {
    const timer = setTimeout(run, ms);
    timer.unref?.();
    job.timers.push(timer);
  }

  /** Looks for the job's processes: its group, and every descendant of a
   * process already seen (also one that left the group or session). */
  #track(job: Job): void {
    if (job.pid === undefined) return;
    let all: PreviewProcessV1[];
    try { all = this.#options.processes(); } catch { return; }
    const byPid = new Map(all.map(entry => [entry.pid, entry]));
    if (!job.seen.has(job.pid) && byPid.has(job.pid)) job.seen.set(job.pid, byPid.get(job.pid)!.started);
    let grew = true;
    while (grew) {
      grew = false;
      for (const entry of all) {
        if (job.seen.has(entry.pid)) continue;
        const parentSeen = job.seen.get(entry.ppid);
        const parent = byPid.get(entry.ppid);
        if (entry.pgid === job.pid || (parentSeen !== undefined && parent?.started === parentSeen)) {
          job.seen.set(entry.pid, entry.started);
          grew = true;
        }
      }
    }
    this.#saveRunning();
  }

  #saveRunning(): void {
    const records = [...this.#jobs.values()].filter(job => job.state === "running" || job.leftovers > 0)
      .map(job => ({ leader: job.pid ?? 0, processes: [...job.seen].map(([pid, started]) => ({ pid, started })) }));
    try { this.#options.saveRunning(records); } catch { /* best effort */ }
  }

  /** Signals the job's group and every process seen (still the same one). */
  #kill(job: Job, signal: NodeJS.Signals): void {
    if (job.pid !== undefined) { try { this.#options.killGroup(job.pid, signal); } catch { /* gone */ } }
    for (const [pid, started] of job.seen) {
      if (pid === job.pid && signal !== "SIGKILL") continue;
      try { this.#options.killProcess(pid, started, signal); } catch { /* gone */ }
    }
  }

  #stop(job: Job, why: "cancelled" | "timedOut"): void {
    if (job.state !== "running" || job.stopping) return;
    job.stopping = why;
    if (job.pid === undefined) { this.#finish(job, null); return; }
    this.#track(job);
    this.#kill(job, "SIGTERM");
    const grace = this.#options.killGraceMs ?? PREVIEW_BUILD_KILL_GRACE_MS;
    this.#later(job, grace, () => { this.#track(job); this.#kill(job, "SIGKILL"); });
    // However it ends, it is not "running" for ever: the leader was killed.
    this.#later(job, grace + (this.#options.pipeGraceMs ?? PREVIEW_BUILD_PIPE_GRACE_MS) + 1_000, () => {
      if (job.state !== "running") return;
      for (const stream of [job.child?.stdout, job.child?.stderr]) stream?.destroy();
      this.#done(job, null, null);
    });
  }

  /** The leader ended: was it a real result? */
  #done(job: Job, code: number | null, plan: PreviewBuildPlanResultV1 | null): void {
    if (job.state !== "running") return;
    if (code !== 0 || job.stopping || !plan) { this.#finish(job, code); return; }
    // Done only if it wrote what is shown and nothing Git would record (I1·I2).
    const startedAt = job.startedAt ?? 0;
    void this.#plan(job.projectId, job.chatId).then(after => {
      if (job.state !== "running") return;
      if (!after || after.refusal === "dependenciesNotIgnored" || after.refusal === "outputNotIgnored") {
        this.#append(job, "결과 일부가 작업 기록에 섞일 수 있어 결과로 쓰지 않았어요 (Git 무시 아님).");
        this.#finish(job, code, "failed");
        return;
      }
      if (job.step === "build" && (after.workspaceIdentity !== plan.workspaceIdentity || !this.#options.outputWritten(plan.workspacePath!, plan.outputDir!, startedAt - 2_000))) {
        this.#append(job, `빌드가 ${plan.outputDir}/index.html 을 새로 만들지 않았어요.`);
        this.#finish(job, code, "failed");
        return;
      }
      if (job.step === "build") job.output = { dir: plan.outputDir!, workspaceIdentity: plan.workspaceIdentity };
      this.#finish(job, code);
    });
  }

  #finish(job: Job, code: number | null, state?: PreviewBuildStateV1): void {
    if (job.state !== "running") return;
    for (const timer of job.timers) clearTimeout(timer);
    job.timers = [];
    clearInterval(job.tracker);
    job.tracker = undefined;
    job.exitCode = code;
    job.endedAt = (this.#options.now ?? Date.now)();
    job.state = state ?? job.stopping ?? (code === 0 ? "succeeded" : "failed");
    job.child = undefined;
    job.leftovers = [...job.seen].filter(([pid, started]) => this.#options.isAlive(pid, started)).length;
    if (job.leftovers > 0) {
      // One last try; what still lives is told and blocks this task (B3).
      this.#kill(job, "SIGKILL");
      job.leftovers = [...job.seen].filter(([pid, started]) => this.#options.isAlive(pid, started)).length;
      if (job.leftovers > 0) this.#append(job, `끝나지 않은 프로그램 ${job.leftovers}개가 남아 있어요. 다시 빌드하기 전에 앱을 다시 시작해 주세요.`);
    }
    this.#saveRunning();
    this.#options.log(`preview ${job.step} ${job.state}`);
    try { this.#options.writeLog(previewBuildLogName(job.projectId, job.chatId), `${job.lines.join("\n")}\n`); } catch { /* best effort */ }
  }

  #append(job: Job, line: string): void {
    job.lines.push(line);
    if (job.lines.length > PREVIEW_BUILD_LOG_MAX_LINES_V1) {
      const drop = job.lines.length - PREVIEW_BUILD_LOG_MAX_LINES_V1;
      job.lines.splice(0, drop);
      job.base += drop;
    }
  }

  #view(key: string, cursor: number): PreviewBuildJobViewV1 {
    const job = this.#jobs.get(key);
    if (!job) return { ...IDLE_VIEW };
    const end = job.base + job.lines.length;
    const from = Math.max(job.base, Math.min(cursor, end));
    return {
      step: job.step, state: job.state, cursor: end, lines: job.lines.slice(from - job.base),
      exitCode: job.exitCode, outputReady: job.state === "succeeded" && job.step === "build" && job.output !== null,
      startedAt: job.startedAt, endedAt: job.endedAt, leftovers: job.leftovers,
    };
  }
}
