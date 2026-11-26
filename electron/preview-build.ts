/**
 * Build-then-preview runner (결과 미리보기 확장 1단계, 2026-11-27).
 *
 * The person asks to install a task's packages or build it; main asks the
 * sidecar what that task's workspace needs (`preview.buildPlan`, read-only,
 * from the chat binding), shows the exact command, program and folder in
 * its own native box, and only on "예" runs it. The page can neither name
 * the folder nor the command nor answer the box (PR #12·#14 rule).
 *
 * - One job per task at a time, at most `PREVIEW_BUILD_MAX_RUNNING` over all.
 * - The process is the leader of its own process group: cancelling, a time
 *   limit, the page going away and quitting end that group (and only it).
 * - The log is masked line by line before it is kept, kept in memory with a
 *   bounded size and written to the app data folder when the job ends.
 * - A build that succeeds records its output folder for that task; the
 *   preview opens it with `source: "build"` (`buildOutput`).
 */
import type { ChildProcess, SpawnOptions } from "node:child_process";
import {
  PREVIEW_BUILD_LOG_MAX_LINES_V1,
  type PreviewBuildIpcRequestV1,
  type PreviewBuildIpcResultV1,
  type PreviewBuildJobViewV1,
  type PreviewBuildPlanResultV1,
  type PreviewBuildPlanViewV1,
  type PreviewBuildStateV1,
  type PreviewBuildStepV1,
} from "../src/m5/contracts/preview-v1.ts";
import {
  PREVIEW_BUILD_KILL_GRACE_MS,
  PREVIEW_BUILD_MAX_RUNNING,
  PREVIEW_BUILD_TIMEOUT_MS,
  PREVIEW_INSTALL_TIMEOUT_MS,
  PreviewLineSplitter,
  previewBuildArgs,
  previewBuildCommandLine,
  previewBuildEnvironment,
  previewBuildLine,
  previewBuildLogName,
} from "./preview-build-policy.ts";

export interface PreviewBuildSidecarV1 {
  previewBuildPlan(input: { projectId: string; chatId: string }): Promise<PreviewBuildPlanResultV1>;
}

/** What the consent box shows. */
export interface PreviewBuildConsentV1 {
  step: PreviewBuildStepV1;
  command: string;
  program: string;
  folder: string;
  buildScript: string | null;
}

export interface PreviewBuildOptionsV1 {
  enabled: boolean;
  sidecar(): PreviewBuildSidecarV1 | undefined;
  /** main's native box; true only for the person's "예". */
  confirm(consent: PreviewBuildConsentV1): Promise<boolean>;
  /** `node` and the package manager, or null when not found on this computer. */
  findTools(manager: "npm" | "pnpm" | "yarn"): { program: string; pathDirs: string[] } | null;
  /** The folder is a real directory, reached without links. */
  isWorkspaceFolder(path: string): boolean;
  spawn(program: string, args: string[], options: SpawnOptions): ChildProcess;
  /** Signals a process group (POSIX `kill(-pgid)`). */
  killGroup(pid: number, signal: NodeJS.Signals): void;
  environment: Readonly<Record<string, string | undefined>>;
  /** Where ended jobs' masked logs are written (best effort). */
  writeLog(name: string, text: string): void;
  log(label: string): void;
  now?(): number;
  timeoutMs?: { install: number; build: number };
  killGraceMs?: number;
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
  timer: ReturnType<typeof setTimeout> | undefined;
  killTimer: ReturnType<typeof setTimeout> | undefined;
  /** Set by a build that succeeded, cleared when another step starts. */
  output: { dir: string; workspaceIdentity: string } | null;
  /** The person asked to stop (or the time ran out) before it ended. */
  stopping: "cancelled" | "timedOut" | null;
}

const IDLE_VIEW: PreviewBuildJobViewV1 = { step: null, state: "idle", cursor: 0, lines: [], exitCode: null, outputReady: false, startedAt: null, endedAt: null };

export class PreviewBuildManagerV1 {
  readonly #options: PreviewBuildOptionsV1;
  readonly #jobs = new Map<string, Job>();
  /** Tasks whose consent box is open: a second ask waits for none. */
  readonly #asking = new Set<string>();
  #quitting = false;

  constructor(options: PreviewBuildOptionsV1) { this.#options = options; }

  /** For tests and probes (never IPC): the live processes' ids. */
  get runningPids(): number[] {
    return [...this.#jobs.values()].filter(job => job.state === "running" && job.pid !== undefined).map(job => job.pid!);
  }

  async request(request: PreviewBuildIpcRequestV1): Promise<PreviewBuildIpcResultV1> {
    if (!this.#options.enabled) throw new Error("preview build is not enabled");
    if (this.#quitting) throw new Error("preview build is closed while the app quits");
    const key = `${request.projectId}\u0000${request.chatId}`;
    switch (request.action) {
      case "plan": return { action: "plan", plan: await this.#planView(request.projectId, request.chatId), job: this.#view(key, Number.MAX_SAFE_INTEGER) };
      case "status": return { action: "status", job: this.#view(key, request.cursor) };
      case "cancel": {
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

  /** The app page reloaded or its process ended, the window closed, or a
   * quit is being prepared: nobody watches the jobs any more. */
  stopAll(): void { for (const job of this.#jobs.values()) this.#stop(job, "cancelled"); }

  /** Quitting: every group is killed now (no grace; main is going away). */
  shutdown(): void {
    this.#quitting = true;
    for (const job of this.#jobs.values()) {
      if (job.state !== "running") continue;
      job.stopping ??= "cancelled";
      if (job.pid !== undefined) this.#signal(job, "SIGKILL");
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
      installCommand: previewBuildCommandLine(plan.packageManager, "install", plan.lockfile),
      buildCommand: previewBuildCommandLine(plan.packageManager, "build", plan.lockfile),
      needsInstall: plan.needsInstall,
      outputDir: plan.outputDir,
    };
  }

  async #start(key: string, projectId: string, chatId: string, step: PreviewBuildStepV1): Promise<PreviewBuildIpcResultV1> {
    const answer = (outcome: "started" | "declined" | "busy" | "refused") => ({ action: "start" as const, outcome, job: this.#view(key, Number.MAX_SAFE_INTEGER) });
    const busy = () => this.#asking.has(key) || this.#jobs.get(key)?.state === "running" || this.#running() >= PREVIEW_BUILD_MAX_RUNNING;
    if (busy()) return answer("busy");
    const plan = await this.#plan(projectId, chatId);
    if (!plan || plan.refusal !== null || !plan.packageManager || !plan.workspacePath || !plan.outputDir) return answer("refused");
    // Building needs the packages first (the page offers installing).
    if (step === "build" && plan.needsInstall) return answer("refused");
    const tools = this.#options.findTools(plan.packageManager);
    if (!tools || !this.#options.isWorkspaceFolder(plan.workspacePath)) return answer("refused");
    const args = previewBuildArgs(plan.packageManager, step, plan.lockfile);
    if (busy()) return answer("busy");
    this.#asking.add(key);
    let agreed = false;
    try {
      agreed = await this.#options.confirm({
        step, command: previewBuildCommandLine(plan.packageManager, step, plan.lockfile), program: tools.program,
        folder: plan.workspacePath, buildScript: step === "build" ? previewBuildLine(plan.buildScript ?? "") : null,
      });
    } finally {
      this.#asking.delete(key);
    }
    if (!agreed) return answer("declined");
    if (this.#quitting) throw new Error("preview build is closed while the app quits");
    if (this.#jobs.get(key)?.state === "running" || this.#running() >= PREVIEW_BUILD_MAX_RUNNING) return answer("busy");
    this.#run(key, projectId, chatId, step, tools, args, plan);
    return answer("started");
  }

  #running(): number { return [...this.#jobs.values()].filter(job => job.state === "running").length; }

  #run(key: string, projectId: string, chatId: string, step: PreviewBuildStepV1, tools: { program: string; pathDirs: string[] }, args: string[], plan: PreviewBuildPlanResultV1): void {
    const now = this.#options.now ?? Date.now;
    const previous = this.#jobs.get(key);
    const job: Job = {
      key, projectId, chatId, step, state: "running", lines: [], base: (previous?.base ?? 0) + (previous?.lines.length ?? 0),
      exitCode: null, startedAt: now(), endedAt: null, child: undefined, pid: undefined, timer: undefined, killTimer: undefined,
      output: null, stopping: null,
    };
    this.#jobs.set(key, job);
    this.#append(job, `$ ${[plan.packageManager, ...args].join(" ")}`);
    let child: ChildProcess;
    try {
      child = this.#options.spawn(tools.program, args, {
        cwd: plan.workspacePath!,
        env: previewBuildEnvironment(this.#options.environment, tools.pathDirs),
        // Its own process group: ending it ends what it started.
        detached: process.platform !== "win32",
        stdio: ["ignore", "pipe", "pipe"],
        shell: false,
        windowsHide: true,
      });
    } catch {
      this.#append(job, "could not start the build program");
      this.#finish(job, null);
      return;
    }
    job.child = child;
    job.pid = child.pid;
    this.#options.log(`preview ${step} started`);
    const out = new PreviewLineSplitter();
    const err = new PreviewLineSplitter();
    child.stdout?.setEncoding("utf8");
    child.stderr?.setEncoding("utf8");
    child.stdout?.on("data", (chunk: string) => { for (const line of out.push(chunk)) this.#append(job, previewBuildLine(line)); });
    child.stderr?.on("data", (chunk: string) => { for (const line of err.push(chunk)) this.#append(job, previewBuildLine(line)); });
    child.on("error", () => {
      if (job.state !== "running") return;
      this.#append(job, "could not start the build program");
      this.#finish(job, null);
    });
    child.on("close", (code: number | null) => {
      for (const line of [...out.flush(), ...err.flush()]) this.#append(job, previewBuildLine(line));
      // Whatever the leader left behind in its group goes with it.
      if (job.pid !== undefined) this.#signal(job, "SIGKILL");
      this.#finish(job, code);
      if (job.state === "succeeded" && step === "build") job.output = { dir: plan.outputDir!, workspaceIdentity: plan.workspaceIdentity };
    });
    const limits = this.#options.timeoutMs ?? { install: PREVIEW_INSTALL_TIMEOUT_MS, build: PREVIEW_BUILD_TIMEOUT_MS };
    job.timer = setTimeout(() => this.#stop(job, "timedOut"), step === "install" ? limits.install : limits.build);
  }

  #stop(job: Job, why: "cancelled" | "timedOut"): void {
    if (job.state !== "running" || job.stopping) return;
    job.stopping = why;
    if (job.pid === undefined) { this.#finish(job, null); return; }
    this.#signal(job, "SIGTERM");
    job.killTimer = setTimeout(() => this.#signal(job, "SIGKILL"), this.#options.killGraceMs ?? PREVIEW_BUILD_KILL_GRACE_MS);
  }

  #signal(job: Job, signal: NodeJS.Signals): void {
    try { this.#options.killGroup(job.pid!, signal); } catch { /* already gone */ }
  }

  #finish(job: Job, code: number | null): void {
    if (job.state !== "running") return;
    clearTimeout(job.timer);
    clearTimeout(job.killTimer);
    job.timer = undefined;
    job.killTimer = undefined;
    job.exitCode = code;
    job.endedAt = (this.#options.now ?? Date.now)();
    job.state = job.stopping ?? (code === 0 ? "succeeded" : "failed");
    job.child = undefined;
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
      startedAt: job.startedAt, endedAt: job.endedAt,
    };
  }
}
