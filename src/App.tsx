import { useEffect, useRef, useState } from "react";
import { Channel, invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { openPath, openUrl } from "@tauri-apps/plugin-opener";
import { useI18n } from "./i18n";
import { diagnoseError, toAppError, type AppError } from "./doctor";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { getBaseDir, setBaseDir, saveProject, lastOpenedMap } from "./store";
import type { MessageKey } from "./locales/ko";
import {
  canStartPilot,
  chooseTarget,
  initialTarget,
  retainTarget,
} from "./handoffTarget";
import {
  NO_RUN,
  ownsStage,
  requestCancel,
  runReserved,
  runSettled,
  startingRun,
  type RunTracker,
} from "./handoffRun";
import "./App.css";

interface ToolInfo {
  path: string;
  version: string;
  inShellPath: boolean;
}

interface EnvironmentReport {
  os: string;
  arch: string;
  osVersion: string | null;
  agent: ToolInfo | null;
  node: ToolInfo | null;
  checkedPaths: string[];
}

interface InstallResult {
  version: string;
  path: string;
  profileUpdated: string | null;
}

type InstallEvent =
  | { type: "phase"; name: string }
  | { type: "log"; line: string }
  | { type: "progress"; downloadedBytes: number };

interface LoginStatus {
  loggedIn: boolean;
  authMethod: string | null;
  email: string | null;
  subscriptionType: string | null;
}

type LoginEvent =
  | { type: "url"; url: string }
  | { type: "log"; line: string }
  | { type: "exit"; success: boolean };

/** 안전 프리셋을 앱이 새로 썼는지("applied"), 기존 설정을 그대로 뒀는지("kept") */
type PresetOutcome = "applied" | "kept";

interface ProjectInfo {
  path: string;
  created: boolean;
  safety: {
    /** CLAUDE.md · AGENTS.md */
    guide: PresetOutcome;
    /** .claude/settings.json · ~/.codex/config.toml */
    config: PresetOutcome;
  };
}

interface EditorInfo {
  id: string;
  name: string;
  url: string;
  installed: boolean;
}

interface AgentStatus {
  installed: boolean;
  version: string | null;
  loggedIn: boolean;
  path: string | null;
  /** 설치는 됐는데 온전하지 않다 — 다시 설치하면 고쳐진다 */
  needsRepair: boolean;
}

// 기준 폴더 스캔으로 발견한 프로젝트 (앱 기억이 아니라 디스크 실제 폴더)
interface ScannedProject {
  path: string;
  name: string;
  agent: string;
  /**
   * 안전한 작업 사본을 만들 수 있는가 — **백엔드가 실행과 같은 기준으로 채운다.**
   * 에이전트 표식이 있다고 파일럿을 쓸 수 있는 건 아니다(복제본은 Git 클론이다).
   */
  pilotEligible: boolean;
}
interface HomeProject extends ScannedProject {
  lastOpenedAt: number;
}

// 기준 폴더를 스캔해 프로젝트 목록을 만들고, store의 최근 사용 시각으로 정렬한다.
async function loadHomeProjects(base: string): Promise<HomeProject[]> {
  const [scanned, last] = await Promise.all([
    invoke<ScannedProject[]>("scan_projects", { base }),
    lastOpenedMap(),
  ]);
  return scanned
    .map((s) => ({ ...s, lastOpenedAt: last[s.path] ?? 0 }))
    .sort(
      (a, b) =>
        b.lastOpenedAt - a.lastOpenedAt || a.name.localeCompare(b.name),
    );
}

async function openDirectoryDialog(): Promise<string | null> {
  try {
    const picked = await openDialog({ directory: true, multiple: false });
    return typeof picked === "string" ? picked : null;
  } catch {
    return null;
  }
}

const shortVersion = (v: string | null) => v?.split(" ")[0] ?? "";

// 설치·로그인까지 끝난 에이전트 목록. 홈 진입과 새 프로젝트 단축에 함께 쓴다.
async function getReadyAgents(): Promise<AgentId[]> {
  try {
    const ids: AgentId[] = ["claude-code", "codex"];
    const results = await Promise.all(
      ids.map((id) =>
        invoke<AgentStatus>("agent_status", { agent: id }).catch(() => null),
      ),
    );
    return ids.filter((_, index) => {
      const status = results[index];
      return status?.installed && status.loggedIn;
    });
  } catch {
    return [];
  }
}

// "x.y.z"를 뽑아 숫자 배열로. 최신 > 설치 판정용.
function semverParts(s: string): number[] | null {
  const m = s.match(/(\d+)\.(\d+)\.(\d+)/);
  return m ? [Number(m[1]), Number(m[2]), Number(m[3])] : null;
}
function isNewer(latest: string, installed: string): boolean {
  const a = semverParts(latest);
  const b = semverParts(installed);
  if (!a || !b) return false;
  for (let i = 0; i < 3; i++) {
    if (a[i] > b[i]) return true;
    if (a[i] < b[i]) return false;
  }
  return false;
}

type AgentId = "claude-code" | "codex";

const AGENTS: { id: AgentId; vendor: string; recommended?: boolean }[] = [
  { id: "claude-code", vendor: "Anthropic", recommended: true },
  { id: "codex", vendor: "OpenAI" },
];

const STEP_KEYS = [
  "agent",
  "diagnosis",
  "install",
  "login",
  "project",
  "graduation",
] as const;

const INSTALL_PHASE_IDS = ["download", "install", "path", "verify"] as const;
const KNOWN_PLANS = ["pro", "max", "team", "enterprise"];

// 편집기 표시명은 언어 리소스로 (백엔드가 준 name 대신 id 기준)
const editorLabel = (t: (k: MessageKey) => string, id: string) =>
  id === "cursor" || id === "vscode" ? t(`editor.${id}` as MessageKey) : id;

function osLabel(report: EnvironmentReport): string {
  if (report.os === "macos") {
    const chip = report.arch === "aarch64" ? "Apple Silicon" : "Intel";
    const ver = report.osVersion ? ` ${report.osVersion}` : "";
    return `macOS${ver} (${chip})`;
  }
  if (report.os === "windows") return "Windows";
  return `${report.os} (${report.arch})`;
}

// 닥터: 에러·로그를 초보자용 진단으로 풀어주고 재시도를 돕는다 (M3 — 삽질 분류 G)
function DoctorCard({
  error,
  log,
  onRetry,
  retryLabel,
}: {
  error: AppError;
  log?: string[];
  onRetry?: () => void;
  retryLabel?: string;
}) {
  const { t } = useI18n();
  const dx = diagnoseError(error, log);
  const detail = [error.detail, ...(log ?? [])].filter(Boolean).join("\n");
  return (
    <div className="doctor">
      <span className="doctor-badge">{t("doctor.badge")}</span>
      <strong>{t(dx.titleKey)}</strong>
      <p>{t(dx.adviceKey)}</p>
      {onRetry && (
        <button className="primary" onClick={onRetry}>
          {retryLabel ?? t("doctor.retry")}
        </button>
      )}
      {detail && (
        <details className="doctor-details">
          <summary>{t("doctor.details")}</summary>
          <pre>{detail}</pre>
        </details>
      )}
    </div>
  );
}

type View = "loading" | "home" | "wizard";

function App() {
  const { t, lang, setLang } = useI18n();
  const [view, setView] = useState<View>("loading");
  const [projects, setProjects] = useState<HomeProject[]>([]);
  const [baseDir, setBaseDirState] = useState<string>("");
  const [scanError, setScanError] = useState<AppError | null>(null);
  const [readyAgents, setReadyAgents] = useState<AgentId[]>([]);
  const [step, setStep] = useState(0);
  const [agent, setAgent] = useState<AgentId | null>(null);
  const [report, setReport] = useState<EnvironmentReport | null>(null);
  const [project, setProject] = useState<ProjectInfo | null>(null);

  // 홈 진입 조건: 기준 폴더에 실제 프로젝트 폴더가 있거나, 이미 에이전트를
  // 설치·로그인해 둔 경우(이미 준비된 사용자는 온보딩 불필요). 둘 다 아니면 위저드.
  async function refreshHome() {
    let base = await getBaseDir();
    if (!base) base = await invoke<string>("default_projects_dir");
    setBaseDirState(base);
    let list: HomeProject[] = [];
    let nextScanError: AppError | null = null;
    try {
      list = await loadHomeProjects(base);
    } catch (error) {
      nextScanError = toAppError(error);
    }
    const nextReadyAgents = await getReadyAgents();
    setProjects(list);
    setScanError(nextScanError);
    setReadyAgents(nextReadyAgents);
    if (list.length > 0 || nextReadyAgents.length > 0 || nextScanError) {
      setView("home");
      return;
    }
    setView("wizard");
  }
  useEffect(() => {
    refreshHome();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  async function changeBaseDir() {
    const picked = await openDirectoryDialog();
    if (!picked) return;
    await setBaseDir(picked);
    await refreshHome();
  }

  function selectAgent(id: AgentId) {
    setAgent(id);
    setReport(null);
    setProject(null);
    setStep(readyAgents.includes(id) ? 4 : 1);
  }

  function startWizard() {
    const onlyReadyAgent = readyAgents.length === 1 ? readyAgents[0] : null;
    setAgent(onlyReadyAgent);
    setReport(null);
    setProject(null);
    // 준비된 에이전트가 하나면 진단·설치·로그인을 다시 거치지 않고 프로젝트로.
    setStep(onlyReadyAgent ? 4 : 0);
    setView("wizard");
  }

  // 홈의 "내 에이전트"에서 특정 에이전트 설정으로 진입 — 진단부터(설치·로그인은 자동 스킵)
  function setupAgent(id: AgentId) {
    setAgent(id);
    setReport(null);
    setProject(null);
    setStep(1);
    setView("wizard");
  }

  async function goHome() {
    await refreshHome();
    setView("home");
  }

  // 배포 빌드에서 웹뷰 우클릭 메뉴를 막는다(입력창은 붙여넣기 위해 예외).
  // 개발 중에는 devtools 우클릭이 필요하므로 유지한다.
  useEffect(() => {
    if (!import.meta.env.PROD) return;
    const block = (e: MouseEvent) => {
      const el = e.target as HTMLElement;
      if (el.tagName === "INPUT" || el.tagName === "TEXTAREA") return;
      e.preventDefault();
    };
    document.addEventListener("contextmenu", block);
    return () => document.removeEventListener("contextmenu", block);
  }, []);

  return (
    <div className="app">
      <header className="header">
        <div className="lang-toggle">
          <button
            className={lang === "ko" ? "active" : ""}
            onClick={() => setLang("ko")}
          >
            {t("lang.ko")}
          </button>
          <button
            className={lang === "en" ? "active" : ""}
            onClick={() => setLang("en")}
          >
            {t("lang.en")}
          </button>
        </div>
        <p className="eyebrow">{t("app.eyebrow")}</p>
        <h1>Hello, Agent</h1>
        <p className="tagline">{t("app.tagline")}</p>
      </header>

      <AppUpdateBanner />

      {view === "loading" ? (
        <main className="panel" />
      ) : view === "home" ? (
        <HomeView
          projects={projects}
          baseDir={baseDir}
          scanError={scanError}
          onNew={startWizard}
          onSetupAgent={setupAgent}
          onChangeBase={changeBaseDir}
          onChanged={refreshHome}
          onRetryScan={refreshHome}
        />
      ) : (
        <>
          <ol className="steps">
            {STEP_KEYS.map((key, i) => (
              <li
                key={key}
                className={i === step ? "active" : i < step ? "done" : ""}
              >
                <span className="step-dot">{i < step ? "✓" : i + 1}</span>
                {t(`steps.${key}` as MessageKey)}
              </li>
            ))}
          </ol>

          <main className="panel" key={step}>
            {step === 0 || !agent ? (
              <AgentStep selected={agent} onSelect={selectAgent} />
            ) : step === 1 ? (
              <DiagnosisStep
                agent={agent}
                report={report}
                onReport={setReport}
                onNext={() => setStep(2)}
              />
            ) : step === 2 ? (
              <InstallStep
                agent={agent}
                report={report}
                onNext={() => setStep(3)}
              />
            ) : step === 3 ? (
              <LoginStep agent={agent} onNext={() => setStep(4)} />
            ) : step === 4 ? (
              <ProjectStep
                agent={agent}
                project={project}
                onProject={setProject}
                onNext={() => setStep(5)}
              />
            ) : (
              <GraduationStep
                agent={agent}
                project={project}
                onHome={goHome}
              />
            )}
          </main>
        </>
      )}
    </div>
  );
}

// 상주 에이전트 상태: 설치·로그인·업데이트를 홈에서 한눈에 (일회성 탈출의 "재방문 이유")
function AgentRow({ id, onSetup }: { id: AgentId; onSetup: () => void }) {
  const { t } = useI18n();
  const [status, setStatus] = useState<AgentStatus | null>(null);
  const [latest, setLatest] = useState<string | null>(null);
  const [updating, setUpdating] = useState(false);
  const [error, setError] = useState<AppError | null>(null);
  const name = t(`agent.${id}.name` as MessageKey);

  async function refresh() {
    setError(null);
    setLatest(null);
    try {
      const s = await invoke<AgentStatus>("agent_status", { agent: id });
      setStatus(s);
      // 설치돼 있으면 백그라운드로 최신 버전 확인(느릴 수 있음, 실패는 조용히 무시)
      if (s.installed && s.version) {
        invoke<string | null>("latest_agent_version", { agent: id })
          .then((v) => setLatest(v))
          .catch(() => {});
      }
    } catch (caught) {
      setStatus(null);
      setError(toAppError(caught));
    }
  }

  useEffect(() => {
    refresh();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [id]);

  async function update() {
    setUpdating(true);
    setError(null);
    const onEvent = new Channel();
    try {
      await invoke("install_agent", { agent: id, testHome: null, onEvent });
      setLatest(null);
      await refresh();
    } catch (caught) {
      setError(toAppError(caught));
    } finally {
      setUpdating(false);
    }
  }

  const updateAvailable =
    !!status?.installed &&
    !!status.version &&
    !!latest &&
    isNewer(latest, status.version);

  let statusText: string;
  let statusClass: string;
  if (!status) {
    statusText = t("home.agent.checking");
    statusClass = "";
  } else if (!status.installed) {
    statusText = t("home.agent.notInstalled");
    statusClass = "warn";
  } else if (!status.loggedIn) {
    statusText = t("home.agent.needLogin");
    statusClass = "warn";
  } else if (status.needsRepair) {
    // 버전은 최신인데 실제로는 못 쓰는 상태. 이걸 "설치됨"이라 말하면 손쓸 방법이 없다
    statusText = t("home.agent.needsRepair");
    statusClass = "warn";
  } else if (updateAvailable) {
    statusText = t("home.agent.updateAvailable", {
      version: semverParts(latest!)!.join("."),
    });
    statusClass = "update";
  } else {
    statusText = t("home.agent.installed", {
      version: shortVersion(status.version),
    });
    statusClass = "ok";
  }

  return (
    <div className="agent-row-block">
      <div className="agent-row">
        <div className="agent-row-info">
          <strong>{name}</strong>
          <span className={`agent-status ${statusClass}`}>{statusText}</span>
        </div>
        {status && !status.installed ? (
          <button className="ghost small" onClick={onSetup}>
            {t("home.agent.setup")}
          </button>
        ) : status && !status.loggedIn ? (
          <button className="ghost small" onClick={onSetup}>
            {t("home.agent.setup")}
          </button>
        ) : status && status.needsRepair ? (
          <button className="primary small" onClick={update} disabled={updating}>
            {updating ? t("home.agent.repairing") : t("home.agent.repair")}
          </button>
        ) : updateAvailable ? (
          <button className="primary small" onClick={update} disabled={updating}>
            {updating ? t("home.agent.updating") : t("home.agent.update")}
          </button>
        ) : null}
      </div>
      {error && <DoctorCard error={error} onRetry={status ? update : refresh} />}
    </div>
  );
}

// Hello, Agent 앱 자체의 자동 업데이트 배너. 시작 시 새 버전을 확인하고, 있으면
// 한 번의 클릭으로 내려받아 설치·재시작한다(홈의 에이전트 업데이트와는 별개다).
function AppUpdateBanner() {
  const { t } = useI18n();
  const [update, setUpdate] = useState<Update | null>(null);
  const [busy, setBusy] = useState(false);
  const [failed, setFailed] = useState(false);

  useEffect(() => {
    // 확인 실패(오프라인·미설정 등)는 조용히 무시한다.
    check()
      .then((u) => {
        if (u) setUpdate(u);
      })
      .catch(() => {});
  }, []);

  if (!update) return null;

  async function apply() {
    setBusy(true);
    setFailed(false);
    try {
      await update!.downloadAndInstall();
      await relaunch();
    } catch {
      setFailed(true);
      setBusy(false);
    }
  }

  return (
    <div className="app-update-banner">
      <div className="app-update-text">
        <strong>{t("appUpdate.title", { version: update.version })}</strong>
        <span>{failed ? t("appUpdate.failed") : t("appUpdate.desc")}</span>
      </div>
      <button className="primary small" onClick={apply} disabled={busy}>
        {busy ? t("appUpdate.updating") : t("appUpdate.update")}
      </button>
    </div>
  );
}

// 초보자용 코딩 에이전트 사용 팁. 졸업식(첫 대화 직후)과 홈에서 함께 쓴다.
// agent를 넘기면 해당 에이전트의 특징만, 없으면(홈) 둘 다 보여준다.
function UsageTips({ agent }: { agent?: AgentId }) {
  const { t } = useI18n();
  return (
    <div className="usage-tips">
      <p className="tips-lead">{t("tips.try")}</p>
      <ul className="tips-examples">
        <li>{t("tips.ex1")}</li>
        <li>{t("tips.ex2")}</li>
        <li>{t("tips.ex3")}</li>
      </ul>

      <p className="tips-lead">{t("tips.know.title")}</p>
      <ul className="tips-know">
        <li>{t("tips.know1")}</li>
        {/* 승인 동작이 에이전트마다 다르므로 하나로 뭉뚱그리지 않는다 */}
        <li>
          {agent
            ? t(`tips.know2.${agent}` as MessageKey)
            : t("tips.know2.general")}
        </li>
        <li>{t("tips.know3")}</li>
      </ul>

      <p className="tips-lead">{t("tips.agent.title")}</p>
      <ul className="tips-know">
        {agent ? (
          <li>{t(`tips.agent.${agent}` as MessageKey)}</li>
        ) : (
          <>
            <li>
              <strong>Claude Code</strong> — {t("tips.agent.claude-code")}
            </li>
            <li>
              <strong>Codex</strong> — {t("tips.agent.codex")}
            </li>
          </>
        )}
      </ul>

      <p className="tips-lead">{t("tips.context.title")}</p>
      <ul className="tips-know">
        <li>{t("tips.context.desc1")}</li>
        <li>{t("tips.context.desc2")}</li>
      </ul>
    </div>
  );
}

function HomeView({
  projects,
  baseDir,
  scanError,
  onNew,
  onSetupAgent,
  onChangeBase,
  onChanged,
  onRetryScan,
}: {
  projects: HomeProject[];
  baseDir: string;
  scanError: AppError | null;
  onNew: () => void;
  onSetupAgent: (id: AgentId) => void;
  onChangeBase: () => void;
  onChanged: () => void;
  onRetryScan: () => void;
}) {
  const { t, lang } = useI18n();
  const [opening, setOpening] = useState<string | null>(null);
  /**
   * 자동 인계를 맡길 프로젝트. **자동으로 정해지지 않는다** —
   * 사용자가 카드에서 고르기 전까지 파일럿은 열리지 않는다.
   */
  const [pilotTarget, setPilotTarget] = useState<string | null>(initialTarget());
  /** "되돌릴 수 있게 준비하기" 확인을 기다리는 프로젝트. 확인 없이는 원본을 건드리지 않는다. */
  const [preparing, setPreparing] = useState<HomeProject | null>(null);
  const [preparingBusy, setPreparingBusy] = useState(false);
  const [prepareError, setPrepareError] = useState<AppError | null>(null);

  // 목록이 새로고침되면 고른 프로젝트가 사라졌거나 자격을 잃었는지 다시 본다
  useEffect(() => {
    setPilotTarget((current) => retainTarget(projects, current));
  }, [projects]);

  const pilotProject = projects.find((p) => p.path === pilotTarget) ?? null;

  /** 사용자가 확인 창에서 "준비하기"를 눌렀다. */
  async function prepareProject(project: HomeProject) {
    setPreparingBusy(true);
    setPrepareError(null);
    try {
      await invoke("prepare_project_for_handoff", {
        projectPath: project.path,
        language: lang,
        confirmPrepareProject: true,
      });
      setPreparing(null);
      // 목록을 다시 읽어야 자격이 갱신된다
      onChanged();
    } catch (e) {
      setPrepareError(toAppError(e));
    } finally {
      setPreparingBusy(false);
    }
  }

  async function open(p: HomeProject) {
    setOpening(p.path);
    try {
      await openPath(p.path);
      // 최근 사용 시각 기록(정렬용)
      await saveProject({ path: p.path, agent: p.agent, name: p.name });
      onChanged();
    } catch {
      onChanged();
    } finally {
      setOpening(null);
    }
  }

  const fmtWhen = (ts: number) =>
    new Intl.DateTimeFormat(lang === "ko" ? "ko-KR" : "en-US", {
      dateStyle: "medium",
    }).format(ts);

  return (
    <main className="panel">
      <section className="agents-panel">
        <h3>{t("home.agents.title")}</h3>
        {AGENTS.map((a) => (
          <AgentRow key={a.id} id={a.id} onSetup={() => onSetupAgent(a.id)} />
        ))}
      </section>

      <div className="home-head">
        <div>
          <h2>{t("home.title")}</h2>
          <p className="muted">{t("home.subtitle")}</p>
        </div>
        <button className="primary" onClick={onNew}>
          {t("home.new")}
        </button>
      </div>

      <div className="base-row">
        <span className="base-label">{t("home.base.label")}</span>
        <code className="base-path">{baseDir}</code>
        <button className="link" onClick={onChangeBase}>
          {t("home.base.change")}
        </button>
      </div>

      {scanError ? (
        <div className="home-scan-error">
          <DoctorCard error={scanError} onRetry={onRetryScan} />
          <button className="ghost" onClick={onChangeBase}>
            {t("home.base.chooseAnother")}
          </button>
        </div>
      ) : projects.length === 0 ? (
        <p className="muted home-empty">{t("home.empty")}</p>
      ) : (
        <ul className="project-list">
          {projects.map((p) => (
            <li key={p.path} className="project-card">
              <div className="project-info">
                <strong>{p.name}</strong>
                <span className="project-meta">
                  {t(`home.agent.${p.agent}` as MessageKey)}
                  {p.lastOpenedAt > 0
                    ? ` · ${t("home.lastOpened", { when: fmtWhen(p.lastOpenedAt) })}`
                    : ""}
                </span>
                <code className="project-path">{p.path}</code>
              </div>
              <div className="project-actions">
                <button
                  className="primary"
                  onClick={() => open(p)}
                  disabled={opening !== null}
                >
                  {opening === p.path ? t("home.opening") : t("home.open")}
                </button>
                {/* 맡길 수 있는 프로젝트에만 버튼을 준다. 쓸 수 없으면
                    누르게 해 놓고 거절하는 대신 **이유를 먼저 알려 준다.** */}
                {p.pilotEligible ? (
                  <button
                    className="ghost"
                    onClick={() => setPilotTarget(chooseTarget(projects, p.path))}
                    disabled={pilotTarget === p.path}
                  >
                    {pilotTarget === p.path
                      ? t("home.handoff.picked")
                      : t("home.handoff.pick")}
                  </button>
                ) : (
                  <button
                    className="ghost"
                    onClick={() => {
                      setPrepareError(null);
                      setPreparing(p);
                    }}
                  >
                    {t("home.handoff.prepare")}
                  </button>
                )}
              </div>
            </li>
          ))}
        </ul>
      )}

      {/* 원본 폴더를 건드리는 유일한 동작이다. **무엇이 생기는지 먼저 보여 준다.** */}
      {preparing && (
        <div className="prepare-card">
          <h4>{t("home.prepare.title", { name: preparing.name })}</h4>
          <p>{t("home.prepare.desc")}</p>
          <ul className="prepare-facts">
            <li>{t("home.prepare.keepsFiles")}</li>
            <li>{t("home.prepare.addsFolder")}</li>
          </ul>
          {prepareError && <DoctorCard error={prepareError} />}
          <div className="prepare-actions">
            <button
              className="primary"
              onClick={() => prepareProject(preparing)}
              disabled={preparingBusy}
            >
              {preparingBusy ? t("home.prepare.working") : t("home.prepare.confirm")}
            </button>
            <button
              className="ghost"
              onClick={() => setPreparing(null)}
              disabled={preparingBusy}
            >
              {t("home.prepare.cancel")}
            </button>
          </div>
        </div>
      )}

      {/* 사용자가 고른 프로젝트에만 열린다 — `projects[0]`을 말없이 쓰지 않는다 */}
      {pilotProject && (
        <HandoffPilot
          project={pilotProject}
          onClose={() => setPilotTarget(null)}
        />
      )}

      <details className="tips-card">
        <summary>{t("home.tips.title")}</summary>
        <UsageTips />
      </details>
    </main>
  );
}

/**
 * §7.10 최소 자동 인계 파일럿 (macOS 전용).
 *
 * 화면 계약은 §7.10.2다 — **오케스트레이션을 노출하지 않는다.**
 * 기본 화면에는 목표·진행 상태·결과 요약·변경 내용·보관/폐기만 둔다.
 * 세션 ID·증거 경로·권한 플래그·역할 선택은 전부 "자세히 보기" 뒤로 보낸다.
 */
type HandoffStage =
  | "organizing"
  | "building"
  | "checking"
  | "done"
  /** 무엇을 바꿀지 알아야 한다 — 사고가 아니라 되묻기 */
  | "needsGoal"
  | "needsAttention";

/** 화면 단계. 백엔드 단계에 **준비**만 더한다 — 준비는 이 화면이 직접 하는 일이다. */
type ViewStage = "idle" | "preparing" | HandoffStage;

/** 복제본이 잠긴 방식. `sessionOnly`는 앱을 껐다 켜면 잠금이 풀린다는 뜻이다. */
type WorkspaceLock = "none" | "persisted" | "sessionOnly";

type ChangeKind =
  | "added"
  | "modified"
  | "deleted"
  | "renamed"
  | "copied"
  | "other";

interface ChangedFile {
  path: string;
  kind: ChangeKind;
  from: string | null;
  status: string;
}

interface HandoffEvidence {
  claudeEventsPath: string;
  claudeStderrPath: string;
  claudeDiffPath: string;
  claudeSessionId: string;
  claudeCliVersion: string;
  codexEventsPath: string;
  codexLastMessagePath: string;
  codexStderrPath: string;
  codexDiffPath: string;
  codexSessionId: string;
  codexRunId: string;
  permissionDenials: string[];
  termination: string;
}

/** 앞선 실행이 남긴 잠긴 사본. 이걸 정리하기 전에는 새 실행이 시작되지 않는다. */
interface BlockedWorkspace {
  workspacePath: string;
  reason: string;
  /** 잠금이 디스크에도 남아 있는가 — 다시 확인(복구)을 시도할 수 있는지 가른다 */
  persisted: boolean;
  markerError: string | null;
}

type HandoffStart =
  | { state: "ready"; runId: string }
  | { state: "blocked"; blocker: BlockedWorkspace };

interface HandoffResult {
  stage: HandoffStage;
  goal: string;
  /** 하려던 것 — 결과가 아니다 */
  planSummary: string;
  /** 실제로 한 것 */
  resultSummary: string;
  changedFiles: ChangedFile[];
  attention: string | null;
  workspaceLock: WorkspaceLock;
  lockError: string | null;
  blocker: BlockedWorkspace | null;
  workspacePath: string;
  evidence: HandoffEvidence;
}

const STAGE_ORDER: ViewStage[] = [
  "preparing",
  "organizing",
  "building",
  "checking",
  "done",
];

/**
 * 각 단계를 어떻게 그릴지.
 *
 * 멈춘 경우 **진행이 초기화되면 안 된다** — 어디까지 갔다가 어디서 멈췄는지가
 * 사용자가 다음에 무엇을 할지 정하는 근거다.
 */
function stepState(
  index: number,
  reached: number,
  stage: ViewStage,
): "done" | "active" | "stopped" | "todo" {
  if (stage === "done") return "done";
  if (index < reached) return "done";
  if (index > reached) return "todo";
  if (stage === "needsAttention") return "stopped";
  // 되묻기는 사고가 아니다. 빨갛게 칠하지 않는다
  return stage === "needsGoal" ? "done" : "active";
}

function HandoffPilot({
  project,
  onClose,
}: {
  project: HomeProject;
  onClose: () => void;
}) {
  const { t } = useI18n();
  const [available, setAvailable] = useState(false);
  const [goal, setGoal] = useState("");
  const [stage, setStage] = useState<ViewStage>("idle");
  // 어디까지 갔는가. 멈춰도 이 값은 유지된다 — 멈춘 지점을 보여 주려면 필요하다.
  const [reached, setReached] = useState<ViewStage>("preparing");
  const [result, setResult] = useState<HandoffResult | null>(null);
  const [error, setError] = useState<AppError | null>(null);
  const [cleanup, setCleanup] = useState<"none" | "working" | "discarded" | "kept">(
    "none",
  );
  /** 앞선 사본이 잠겨 새 실행을 시작할 수 없는 상태. 이것부터 치워야 한다. */
  const [blocker, setBlocker] = useState<BlockedWorkspace | null>(null);
  const [clearing, setClearing] = useState(false);
  /**
   * 지금 따라가는 실행과, 아직 전달하지 못한 중단.
   *
   * 상태 전이는 `handoffRun.ts`의 순수 함수가 정하고 여기서는 배선만 한다 —
   * ref 두 개를 손으로 굴리면 "끝난 실행 ID가 남아 새 실행의 중단을 삼키는" 경합이 난다.
   */
  const runRef = useRef<RunTracker>(NO_RUN);

  useEffect(() => {
    invoke<boolean>("handoff_available")
      .then(setAvailable)
      .catch(() => setAvailable(false));
  }, []);

  // 진행 상태는 백엔드가 알려 준다. 사용자는 어느 에이전트가 도는지 몰라도 된다.
  useEffect(() => {
    const unlisten = listen<{ runId: string; stage: HandoffStage }>(
      "handoff:stage",
      (event) => {
        // 다른(이미 끝난) 실행의 상태는 이 화면의 진행이 아니다
        if (!ownsStage(runRef.current, event.payload.runId)) return;
        setStage(event.payload.stage);
        if (STAGE_ORDER.includes(event.payload.stage)) {
          setReached(event.payload.stage);
        }
      },
    );
    return () => {
      unlisten.then((off) => off());
    };
  }, []);

  if (!available) return null;

  const running =
    stage !== "idle" && stage !== "done" && stage !== "needsAttention";

  async function start() {
    setError(null);
    setResult(null);
    setCleanup("none");
    setStage("preparing");
    setReached("preparing");
    setBlocker(null);
    // ★ **`begin_handoff`를 부르기 전에** 이전 실행 ID를 버린다.
    //   남겨 두면 예약을 기다리는 동안 누른 중단이 "실행 중"으로 오인돼
    //   이미 끝난 슬롯으로 나가고, 새 실행은 그대로 진행된다.
    runRef.current = startingRun();
    let runId = "";
    try {
      // **사본을 만들기 전에** 실행을 예약한다. 그래야 준비 중에 누른 중단이 살아남고,
      // 두 번째 실행이 같은 사본을 건드리는 일도 막힌다.
      // 앞선 사본이 잠겨 있으면 여기서 막힌다 — 새 사본을 만들어 봐야 실행이 거부되고,
      // 화면에는 새 사본만 보여 정작 막고 있는 사본에 손댈 수 없게 된다.
      const started = await invoke<HandoffStart>("begin_handoff");
      if (started.state === "blocked") {
        setBlocker(started.blocker);
        runRef.current = runSettled();
        setStage("idle");
        return;
      }
      runId = started.runId;
      if (!runId) {
        // 백엔드 응답 모양이 바뀌었다. 빈 값을 그대로 다음 호출에 넘기면
        // **인자가 통째로 사라져** 엉뚱한 곳에서 터진다. 여기서 멈추는 게 낫다.
        throw new Error("handoff start reply did not include a run id");
      }
      const reserved = runReserved(runRef.current, runId);
      runRef.current = reserved.next;
      if (reserved.abort) {
        // 예약을 기다리는 동안 중단을 눌렀다. 보류된 취소를 전달하고
        // **사본을 만들기 전에** 멈춘다 — 그만두라는 작업의 사본을 남길 이유가 없다.
        await invoke("cancel_handoff").catch(() => {});
        await invoke("release_handoff", { runId }).catch(() => {});
        runRef.current = runSettled();
        setStage("idle");
        return;
      }
      // 원본이 아니라 **폐기 가능한 사본**에서만 돈다 (S2 경계 그대로)
      const workspace = await invoke<{ path: string }>("prepare_codex_pilot", {
        sourcePath: project.path,
      });
      const outcome = await invoke<HandoffResult>("run_handoff", {
        runId,
        workspacePath: workspace.path,
        // 카드에 보이는 그 에이전트가 실제로 일한다 — 화면과 실행이 갈리면 안 된다
        agent: project.agent,
        goal,
      });
      setResult(outcome);
      setBlocker(outcome.blocker);
      setStage(outcome.stage);
      // 결과를 받았으면 따라갈 실행이 없다
      runRef.current = runSettled();
    } catch (e) {
      // 준비에서 실패했다면 예약이 남는다. 돌려주지 않으면 다음 실행이 영영 막힌다.
      if (runId) {
        await invoke("release_handoff", { runId }).catch(() => {});
      }
      runRef.current = runSettled();
      setError(toAppError(e));
      setStage("idle");
    }
  }

  async function stop() {
    const decision = requestCancel(runRef.current);
    runRef.current = decision.next;
    if (!decision.deliverNow) {
      // 아직 예약 전이다. 슬롯이 생기는 즉시 `start`가 이 요청을 전달한다.
      return;
    }
    try {
      await invoke("cancel_handoff");
    } catch {
      /* 중단 요청 실패는 결과 화면에서 드러난다 */
    }
  }

  /** 막고 있는 사본을 정리한다. 이걸 해야 다음 실행이 열린다. */
  async function clearBlocker(mode: "discard" | "recover") {
    if (!blocker) return;
    setClearing(true);
    setError(null);
    try {
      if (mode === "recover") {
        await invoke("recover_codex_pilot", {
          workspacePath: blocker.workspacePath,
        });
      } else {
        await invoke("discard_handoff_workspace", {
          workspacePath: blocker.workspacePath,
          confirmDiscardWorkspace: true,
        });
      }
      setBlocker(null);
    } catch (e) {
      setError(toAppError(e));
    } finally {
      setClearing(false);
    }
  }

  async function discard() {
    if (!result) return;
    setCleanup("working");
    try {
      // 인계 전용 폐기다. `discard_codex_pilot`은 잠긴 사본만 다뤄서
      // 정상 완료·취소 결과에서는 전부 실패한다.
      await invoke("discard_handoff_workspace", {
        workspacePath: result.workspacePath,
        confirmDiscardWorkspace: true,
      });
      setCleanup("discarded");
    } catch (e) {
      setError(toAppError(e));
      setCleanup("none");
    }
  }

  return (
    <section className="handoff-card">
      <div className="handoff-head">
        <h3>
          {t("handoff.title")} <span className="handoff-badge">{t("handoff.badge")}</span>
        </h3>
        <p className="muted">{t("handoff.subtitle")}</p>
      </div>

      {/* 어느 프로젝트에 맡기는지 화면에서 분명해야 한다 */}
      <p className="handoff-notice">
        {t("handoff.notice", { name: project.name })}
        <button className="link" onClick={onClose} disabled={running}>
          {t("handoff.changeProject")}
        </button>
      </p>

      {/* 목록 새로고침 사이에 자격을 잃을 수 있다. 그때도 폼을 내밀지 않는다 */}
      {!project.pilotEligible && (
        <p className="handoff-locked warn">{t("handoff.notEligible")}</p>
      )}

      <label className="handoff-goal">
        <span>{t("handoff.goal.label")}</span>
        <textarea
          value={goal}
          onChange={(e) => setGoal(e.target.value)}
          placeholder={t("handoff.goal.placeholder")}
          rows={3}
          disabled={running || !project.pilotEligible}
        />
      </label>

      {/* 막고 있는 사본을 먼저 보여 준다 — 이걸 치우지 않으면 새 실행이 시작되지 않는다 */}
      {blocker && (
        <div className="handoff-blocked">
          <h4>{t("handoff.blocked.title")}</h4>
          <p>{t("handoff.blocked.desc")}</p>
          <div className="handoff-blocked-actions">
            <button
              className="primary"
              onClick={() => clearBlocker("discard")}
              disabled={clearing}
            >
              {clearing ? t("handoff.blocked.working") : t("handoff.blocked.discard")}
            </button>
            {blocker.persisted && (
              <button
                className="ghost"
                onClick={() => clearBlocker("recover")}
                disabled={clearing}
              >
                {t("handoff.blocked.recover")}
              </button>
            )}
          </div>
          <details className="handoff-details">
            <summary>{t("handoff.details")}</summary>
            <dl>
              <dt>{t("handoff.blocked.which")}</dt>
              <dd>
                <code className="handoff-path">{blocker.workspacePath}</code>
              </dd>
              <dt>{t("handoff.blocked.reason")}</dt>
              <dd>
                <code>{blocker.reason}</code>
              </dd>
              {blocker.markerError && (
                <>
                  <dt>{t("handoff.details.lockError")}</dt>
                  <dd>
                    <code>{blocker.markerError}</code>
                  </dd>
                </>
              )}
            </dl>
          </details>
        </div>
      )}

      <div className="handoff-actions">
        {running ? (
          <button className="ghost" onClick={stop}>
            {t("handoff.cancel")}
          </button>
        ) : (
          <button
            className="primary"
            onClick={start}
            disabled={!canStartPilot(project, goal) || blocker !== null}
          >
            {t("handoff.start")}
          </button>
        )}
      </div>

      {stage !== "idle" && (
        <ol className="handoff-steps">
          {STAGE_ORDER.map((s, index) => (
            <li
              key={s}
              className={`handoff-step ${stepState(index, STAGE_ORDER.indexOf(reached), stage)}`}
            >
              {t(`handoff.stage.${s}` as MessageKey)}
            </li>
          ))}
        </ol>
      )}

      {error && <DoctorCard error={error} onRetry={start} />}

      {result && (
        <div className="handoff-result">
          {result.stage === "needsGoal" ? (
            // 잘못된 것이 없다. 사고 화면이 아니라 **되묻는** 화면이다
            <>
              <h4>{t("handoff.needsGoal.title")}</h4>
              <p>{t("handoff.needsGoal.desc")}</p>
              <p className="muted">{t("handoff.needsGoal.example")}</p>
              <div className="handoff-actions">
                <button className="primary" onClick={() => setResult(null)}>
                  {t("handoff.needsGoal.retry")}
                </button>
              </div>
            </>
          ) : result.attention ? (
            <>
              <h4>{t("handoff.attention.title")}</h4>
              {/* **왜 멈췄는지 쉬운 말로 알려 준다.** 원문은 닥터의 상세 안에만 남는다 */}
              <DoctorCard error={{ kind: "generic", detail: result.attention }} />
              {result.workspaceLock === "persisted" && (
                <p className="handoff-locked">{t("handoff.attention.locked")}</p>
              )}
              {/* 잠금을 저장하지 못했다 — 앱을 껐다 켜면 풀린다. 지금 정리해야 한다 */}
              {result.workspaceLock === "sessionOnly" && (
                <p className="handoff-locked warn">
                  {t("handoff.attention.lockedSessionOnly")}
                </p>
              )}
            </>
          ) : (
            <>
              <h4>{t("handoff.result.title")}</h4>
              {/* 계획 요약이 아니라 **실제 구현 결과**다 */}
              <p className="handoff-summary">
                {result.resultSummary || t("handoff.result.noSummary")}
              </p>
            </>
          )}

          {result.stage !== "needsGoal" && (
            <>
          <h5>{t("handoff.result.changed")}</h5>
          {result.changedFiles.length === 0 ? (
            <p className="muted">{t("handoff.result.noChanges")}</p>
          ) : (
            <ul className="handoff-files">
              {result.changedFiles.map((f) => (
                <li key={`${f.status}:${f.path}`}>
                  <code>{f.path}</code>
                  <span className={`handoff-change ${f.kind}`}>
                    {t(`handoff.change.${f.kind}` as MessageKey)}
                  </span>
                  {f.from && (
                    <span className="muted">
                      {t("handoff.change.from", { name: f.from })}
                    </span>
                  )}
                </li>
              ))}
            </ul>
          )}

            </>
          )}

          <div className="handoff-keep">
            {cleanup === "discarded" ? (
              <p className="muted">{t("handoff.result.discarded")}</p>
            ) : cleanup === "kept" ? (
              <p className="muted">{t("handoff.result.kept")}</p>
            ) : (
              <>
                <button className="ghost" onClick={() => setCleanup("kept")}>
                  {t("handoff.result.keep")}
                </button>
                <button
                  className="link"
                  onClick={discard}
                  disabled={cleanup === "working"}
                >
                  {cleanup === "working"
                    ? t("handoff.result.discarding")
                    : t("handoff.result.discard")}
                </button>
              </>
            )}
          </div>

          {/* 원시 증거는 접이식 상세 뒤에 둔다 — 경로를 읽게 하는 순간 터미널 사용자를 요구하는 셈이다 */}
          <details className="handoff-details">
            <summary>{t("handoff.details")}</summary>
            <p className="muted">{t("handoff.details.desc")}</p>
            <dl>
              {/* 계획은 **하려던 것**이다. 결과 자리에 두지 않고 여기서만 보여 준다 */}
              {result.planSummary && (
                <>
                  <dt>{t("handoff.details.plan")}</dt>
                  <dd>{result.planSummary}</dd>
                </>
              )}
              {result.lockError && (
                <>
                  <dt>{t("handoff.details.lockError")}</dt>
                  <dd>
                    <code>{result.lockError}</code>
                  </dd>
                </>
              )}
              <dt>{t("handoff.details.session")}</dt>
              <dd>
                <code>{result.evidence.claudeSessionId || "—"}</code>
                {result.evidence.claudeCliVersion
                  ? ` · ${result.evidence.claudeCliVersion}`
                  : ""}
              </dd>
              <dt>{t("handoff.details.termination")}</dt>
              <dd>
                <code>{result.evidence.termination || "—"}</code>
              </dd>
              {result.evidence.permissionDenials.length > 0 && (
                <>
                  <dt>{t("handoff.details.denials")}</dt>
                  <dd>{result.evidence.permissionDenials.join(", ")}</dd>
                </>
              )}
              <dt>{t("handoff.details.evidence")}</dt>
              <dd>
                {result.evidence.codexRunId && (
                  <code className="handoff-path">
                    {t("handoff.details.runId")}: {result.evidence.codexRunId}
                  </code>
                )}
                {/* **실제로 저장된 파일만** 보여 준다 — 없는 경로를 주면 사용자는
                    열리지 않는 경로를 들고 도움을 요청하게 된다 */}
                {[
                  result.evidence.claudeEventsPath,
                  result.evidence.codexEventsPath,
                  result.evidence.codexDiffPath,
                ]
                  .filter((path) => path !== "")
                  .map((path) => (
                    <code className="handoff-path" key={path}>
                      {path}
                    </code>
                  ))}
              </dd>
            </dl>
          </details>
        </div>
      )}
    </section>
  );
}

function AgentStep({
  selected,
  onSelect,
}: {
  selected: AgentId | null;
  onSelect: (id: AgentId) => void;
}) {
  const { t } = useI18n();
  return (
    <div>
      <h2>{t("agent.title")}</h2>
      <p className="muted">{t("agent.intro")}</p>
      <div className="agent-cards">
        {AGENTS.map((a) => (
          <button
            key={a.id}
            className={`agent-card ${selected === a.id ? "selected" : ""}`}
            onClick={() => onSelect(a.id)}
          >
            {a.recommended && <span className="badge">{t("agent.badge")}</span>}
            <strong>{t(`agent.${a.id}.name` as MessageKey)}</strong>
            <span className="agent-vendor">{a.vendor}</span>
            <span className="agent-desc">
              {t(`agent.${a.id}.desc` as MessageKey)}
            </span>
          </button>
        ))}
      </div>
    </div>
  );
}

function DiagnosisStep({
  agent,
  report,
  onReport,
  onNext,
}: {
  agent: AgentId;
  report: EnvironmentReport | null;
  onReport: (r: EnvironmentReport) => void;
  onNext: () => void;
}) {
  const { t } = useI18n();
  const [running, setRunning] = useState(false);
  const [error, setError] = useState<AppError | null>(null);
  const name = t(`agent.${agent}.name` as MessageKey);

  async function run() {
    setRunning(true);
    setError(null);
    try {
      onReport(await invoke<EnvironmentReport>("detect_environment", { agent }));
    } catch (e) {
      setError(toAppError(e));
    } finally {
      setRunning(false);
    }
  }

  if (!report) {
    return (
      <div className="center">
        <h2>{t("diag.intro.title")}</h2>
        <p className="muted">{t("diag.intro.desc", { name })}</p>
        {error ? (
          <DoctorCard error={error} onRetry={run} retryLabel={t("diag.run")} />
        ) : (
          <button className="primary" onClick={run} disabled={running}>
            {running ? t("diag.checking") : t("diag.run")}
          </button>
        )}
      </div>
    );
  }

  const tool = report.agent;
  return (
    <div>
      <h2>{t("diag.done.title")}</h2>
      <ul className="results">
        <li className="result ok">
          <span className="result-icon">💻</span>
          <div>
            <strong>{osLabel(report)}</strong>
            <div className="muted">{t("diag.os.desc")}</div>
          </div>
        </li>
        <li className={`result ${tool ? "ok" : "todo"}`}>
          <span className="result-icon">{tool ? "✅" : "📦"}</span>
          <div>
            <strong>
              {tool
                ? t("diag.agent.installed", {
                    name,
                    version: tool.version.split(" ")[0],
                  })
                : t("diag.agent.missing", { name })}
            </strong>
            <div className="muted">
              {tool
                ? tool.inShellPath
                  ? t("diag.agent.inpath")
                  : t("diag.agent.notinpath")
                : t("diag.agent.willinstall")}
            </div>
          </div>
        </li>
        <li className="result ok">
          <span className="result-icon">{report.node ? "🟢" : "⚪️"}</span>
          <div>
            <strong>
              {report.node
                ? t("diag.node.has", { version: report.node.version })
                : t("diag.node.none")}
            </strong>
            <div className="muted">
              {report.node ? t("diag.node.hasdesc") : t("diag.node.nonedesc")}
            </div>
          </div>
        </li>
      </ul>
      <div className="actions">
        <button className="ghost" onClick={run} disabled={running}>
          {t("diag.recheck")}
        </button>
        <button className="primary" onClick={onNext}>
          {tool ? t("diag.next.skip") : t("diag.next.install")}
        </button>
      </div>
    </div>
  );
}

function InstallStep({
  agent,
  report,
  onNext,
}: {
  agent: AgentId;
  report: EnvironmentReport | null;
  onNext: () => void;
}) {
  const { t } = useI18n();
  const [phase, setPhase] = useState<string | null>(null);
  const [log, setLog] = useState<string[]>([]);
  const [downloaded, setDownloaded] = useState(0);
  const [result, setResult] = useState<InstallResult | null>(null);
  const [error, setError] = useState<AppError | null>(null);
  const running = phase !== null && !result && !error;
  const name = t(`agent.${agent}.name` as MessageKey);

  if (report?.agent && !result) {
    return (
      <div className="center">
        <h2>{t("install.already.title")}</h2>
        <p className="muted">
          {t("install.already.desc", {
            name,
            version: report.agent.version.split(" ")[0],
          })}
        </p>
        <button className="primary" onClick={onNext}>
          {t("install.next")}
        </button>
      </div>
    );
  }

  async function start() {
    setError(null);
    setLog([]);
    setDownloaded(0);
    setResult(null);
    setPhase("download");
    const onEvent = new Channel<InstallEvent>();
    onEvent.onmessage = (e) => {
      if (e.type === "phase") setPhase(e.name);
      else if (e.type === "progress") setDownloaded(e.downloadedBytes);
      else setLog((prev) => [...prev.slice(-199), e.line]);
    };
    try {
      setResult(
        await invoke<InstallResult>("install_agent", {
          agent,
          testHome: null,
          onEvent,
        }),
      );
    } catch (err) {
      setError(toAppError(err));
    }
  }

  if (result) {
    return (
      <div className="center">
        <h2>{t("install.done.title")}</h2>
        <p className="muted">
          {t("install.done.desc", {
            name,
            version: result.version.split(" ")[0],
          })}
          <br />
          {result.profileUpdated
            ? t("install.done.path")
            : t("install.done.nopath")}
        </p>
        <button className="primary" onClick={onNext}>
          {t("install.next")}
        </button>
      </div>
    );
  }

  if (running) {
    const currentIndex = INSTALL_PHASE_IDS.findIndex((p) => p === phase);
    return (
      <div>
        <h2>{t("install.running.title")}</h2>
        <p className="muted">{t("install.running.desc")}</p>
        <ul className="phases">
          {INSTALL_PHASE_IDS.map((id, i) => (
            <li
              key={id}
              className={
                i < currentIndex ? "done" : i === currentIndex ? "active" : ""
              }
            >
              <span className="phase-mark">
                {i < currentIndex ? "✓" : i === currentIndex ? "…" : "•"}
              </span>
              {t(`install.phase.${id}` as MessageKey)}
              {(id === "download" || id === "install") &&
                i === currentIndex &&
                downloaded > 0 && (
                  <span className="phase-detail">
                    {t("install.mb", {
                      mb: Math.round(downloaded / 1024 / 1024),
                    })}
                  </span>
                )}
            </li>
          ))}
        </ul>
        <div className="progress-track">
          <div className="progress-fill" />
        </div>
        {log.length > 0 && (
          <details className="log">
            <summary>{t("install.log.detail")}</summary>
            <pre>{log.join("\n")}</pre>
          </details>
        )}
      </div>
    );
  }

  if (error) {
    return (
      <div className="center">
        <h2>{t("install.fail.title")}</h2>
        <p className="muted">{t("install.fail.desc")}</p>
        <DoctorCard
          error={error}
          log={log}
          onRetry={start}
          retryLabel={t("install.retry")}
        />
      </div>
    );
  }

  return (
    <div className="center">
      <h2>{t("install.start.title", { name })}</h2>
      <p className="muted">{t("install.start.desc")}</p>
      <button className="primary" onClick={start}>
        {t("install.start.btn")}
      </button>
    </div>
  );
}

function LoginStep({ agent, onNext }: { agent: AgentId; onNext: () => void }) {
  const { t } = useI18n();
  const [status, setStatus] = useState<LoginStatus | null>(null);
  const [checking, setChecking] = useState(true);
  const [mode, setMode] = useState<"idle" | "waiting" | "verifying">("idle");
  const [url, setUrl] = useState<string | null>(null);
  const [code, setCode] = useState("");
  const [error, setError] = useState<AppError | null>(null);
  const isClaude = agent === "claude-code";
  const name = t(`agent.${agent}.name` as MessageKey);
  const service = t(isClaude ? "login.service.claude" : "login.service.codex");

  async function check() {
    setChecking(true);
    try {
      setStatus(await invoke<LoginStatus>("login_status", { agent }));
      setError(null);
    } catch (e) {
      setError(toAppError(e));
    } finally {
      setChecking(false);
    }
  }

  useEffect(() => {
    check();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [agent]);

  // 브라우저에 이미 세션이 있으면 코드 없이 자동 승인으로 끝나는 경로가 있으므로,
  // 기다리는 동안 로그인 완료를 폴링으로 감지한다
  useEffect(() => {
    if (mode !== "waiting") return;
    const timer = setInterval(async () => {
      try {
        const s = await invoke<LoginStatus>("login_status", { agent });
        if (s.loggedIn) {
          await invoke("cancel_login").catch(() => {});
          setStatus(s);
          setError(null);
          setMode("idle");
        }
      } catch {
        // 일시적 확인 실패는 무시하고 다음 폴링을 기다린다
      }
    }, 3000);
    return () => clearInterval(timer);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [mode]);

  async function openSession(useApiBilling: boolean) {
    setUrl(null);
    const onEvent = new Channel<LoginEvent>();
    onEvent.onmessage = (e) => {
      if (e.type === "url") setUrl(e.url);
    };
    await invoke("start_login", { agent, useApiBilling, onEvent });
  }

  async function start(useApiBilling: boolean) {
    setError(null);
    setCode("");
    setMode("waiting");
    try {
      await openSession(useApiBilling);
    } catch (err) {
      setError(toAppError(err));
      setMode("idle");
    }
  }

  async function submit() {
    if (!code.trim()) return;
    setMode("verifying");
    setError(null);
    const onEvent = new Channel<LoginEvent>();
    try {
      await invoke("submit_login_code", { agent, onEvent, code });
      await check();
      setMode("idle");
    } catch {
      setCode("");
      try {
        await openSession(false);
        setError({ kind: "generic", detail: t("login.codeFail") });
        setMode("waiting");
      } catch (err2) {
        setError(toAppError(err2));
        setMode("idle");
      }
    }
  }

  async function cancel() {
    await invoke("cancel_login").catch(() => {});
    setMode("idle");
    setUrl(null);
  }

  if (checking) {
    return (
      <div className="center">
        <h2>{t("login.checking")}</h2>
      </div>
    );
  }

  if (status?.loggedIn) {
    const plan = status.subscriptionType
      ? KNOWN_PLANS.includes(status.subscriptionType)
        ? t(`plan.${status.subscriptionType}` as MessageKey)
        : status.subscriptionType
      : null;
    const account = t(isClaude ? "login.account.claude" : "login.account.codex");
    return (
      <div className="center">
        <h2>{t("login.done.title")}</h2>
        <p className="muted">
          {status.email ?? account}
          {plan ? ` · ${plan}` : ""}
          <br />
          {t("login.done.ready", { name })}
        </p>
        <button className="primary" onClick={onNext}>
          {t("login.next")}
        </button>
      </div>
    );
  }

  if (mode === "verifying") {
    return (
      <div className="center">
        <h2>{t("login.verifying.title")}</h2>
        <p className="muted">{t("login.verifying.desc")}</p>
      </div>
    );
  }

  if (mode === "waiting") {
    return (
      <div>
        <h2>{t("login.waiting.title")}</h2>
        <ol className="guide">
          <li>{t("login.waiting.step1", { service })}</li>
          <li>{t("login.waiting.step2")}</li>
          {isClaude && <li>{t("login.waiting.step3")}</li>}
        </ol>
        {url && (
          <p className="muted">
            {t("login.browserFail.pre")}
            <button className="link" onClick={() => openUrl(url)}>
              {t("login.browserFail.link")}
            </button>
          </p>
        )}
        {error && <DoctorCard error={error} onRetry={() => start(false)} />}
        {isClaude && (
          <div className="code-row">
            <input
              className="code-input"
              placeholder={t("login.code.placeholder")}
              value={code}
              onChange={(e) => setCode(e.currentTarget.value)}
              onKeyDown={(e) => e.key === "Enter" && submit()}
            />
            <button className="primary" onClick={submit} disabled={!code.trim()}>
              {t("login.code.confirm")}
            </button>
          </div>
        )}
        <div className="actions">
          <button className="ghost" onClick={cancel}>
            {t("login.restart")}
          </button>
        </div>
      </div>
    );
  }

  // 로그인 방식 선택 + 요금 개념 안내 (M2 — 삽질 분류 A·E)
  return (
    <div>
      <h2>{t("login.method.title", { service })}</h2>
      <p className="muted">{t("login.method.desc")}</p>
      {isClaude ? (
        <>
          <div className="method-box">
            <strong>{t("login.claude.box.title")}</strong>
            <p>{t("login.claude.box.desc")}</p>
          </div>
          <p className="hint">{t("login.claude.hint")}</p>
          {error && <DoctorCard error={error} onRetry={() => start(false)} />}
          <div className="actions">
            <details className="advanced">
              <summary>{t("login.claude.advanced")}</summary>
              <p className="muted">{t("login.claude.advanced.desc")}</p>
              <button className="ghost" onClick={() => start(true)}>
                {t("login.claude.api.btn")}
              </button>
            </details>
            <button className="primary" onClick={() => start(false)}>
              {t("login.claude.sub.btn")}
            </button>
          </div>
        </>
      ) : (
        <>
          <div className="method-box">
            <strong>{t("login.codex.box.title")}</strong>
            <p>{t("login.codex.box.desc")}</p>
          </div>
          {error && <DoctorCard error={error} onRetry={() => start(false)} />}
          <div className="actions">
            <span />
            <button className="primary" onClick={() => start(false)}>
              {t("login.codex.btn")}
            </button>
          </div>
        </>
      )}
    </div>
  );
}

function ProjectStep({
  agent,
  project,
  onProject,
  onNext,
}: {
  agent: AgentId;
  project: ProjectInfo | null;
  onProject: (p: ProjectInfo) => void;
  onNext: () => void;
}) {
  const { t, lang } = useI18n();
  const [name, setName] = useState("my-first-project");
  const [creating, setCreating] = useState(false);
  const [error, setError] = useState<AppError | null>(null);

  async function create() {
    setCreating(true);
    setError(null);
    try {
      const base = (await getBaseDir()) ?? undefined;
      const info = await invoke<ProjectInfo>("create_first_project", {
        agent,
        name,
        base,
        language: lang,
      });
      await saveProject({ path: info.path, agent, name });
      onProject(info);
    } catch (e) {
      setError(toAppError(e));
    } finally {
      setCreating(false);
    }
  }

  if (project) {
    return (
      <div className="center">
        <h2>{t("project.done.title")}</h2>
        <p className="muted">
          {project.created ? t("project.done.created") : t("project.done.reuse")}
          <br />
          {/* 실제로 설정을 쓴 경우에만 "넣어뒀다"고 말한다 (docs/proposals/m4-orchestration.md §3) */}
          {project.safety.config === "applied"
            ? t("project.done.safe.applied")
            : t("project.done.safe.kept")}
        </p>
        <code className="path-box">{project.path}</code>
        <button className="primary" onClick={onNext}>
          {t("project.next")}
        </button>
      </div>
    );
  }

  return (
    <div className="center">
      <h2>{t("project.make.title")}</h2>
      <p className="muted">{t("project.make.desc")}</p>
      {error && <DoctorCard error={error} onRetry={create} />}
      <div className="code-row">
        <input
          className="code-input"
          value={name}
          onChange={(e) => setName(e.currentTarget.value)}
          onKeyDown={(e) => e.key === "Enter" && create()}
          placeholder={t("project.folder.placeholder")}
        />
        <button
          className="primary"
          onClick={create}
          disabled={creating || !name.trim()}
        >
          {creating ? t("project.making") : t("project.make.btn")}
        </button>
      </div>
    </div>
  );
}

function GraduationStep({
  agent,
  project,
  onHome,
}: {
  agent: AgentId;
  project: ProjectInfo | null;
  onHome: () => void;
}) {
  const { t, lang } = useI18n();
  const [running, setRunning] = useState(false);
  const [reply, setReply] = useState<string | null>(null);
  const [error, setError] = useState<AppError | null>(null);
  const [editors, setEditors] = useState<EditorInfo[]>([]);
  const [opening, setOpening] = useState<string | null>(null);
  const name = t(`agent.${agent}.name` as MessageKey);

  async function refreshEditors() {
    try {
      setEditors(await invoke<EditorInfo[]>("detect_editors"));
    } catch {
      // 감지 실패는 무시 — 다음 확인 때 다시 시도한다
    }
  }

  useEffect(() => {
    refreshEditors();
    // 사용자가 편집기를 설치하고 앱으로 돌아오면(창 포커스) 자동으로 다시 감지한다
    window.addEventListener("focus", refreshEditors);
    return () => window.removeEventListener("focus", refreshEditors);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  async function openInEditor(id: string) {
    setOpening(id);
    try {
      await invoke("open_in_editor", { editor: id, agent, path: project!.path });
    } catch {
      // 실패해도 조용히 — 사용자는 폴더 열기 버튼으로 대신할 수 있다
    } finally {
      setOpening(null);
    }
  }

  if (!project) {
    return (
      <div className="center">
        <h2>{t("grad.noproject")}</h2>
      </div>
    );
  }

  const installed = editors.filter((e) => e.installed);

  async function chat() {
    setRunning(true);
    setError(null);
    try {
      setReply(
        await invoke<string>("run_first_chat", {
          agent,
          projectPath: project!.path,
          language: lang,
        }),
      );
    } catch (e) {
      setError(toAppError(e));
    } finally {
      setRunning(false);
    }
  }

  if (reply) {
    const fallback = [
      { id: "cursor", url: "https://cursor.com" },
      { id: "vscode", url: "https://code.visualstudio.com" },
    ];
    return (
      <div className="center">
        <div className="confetti" aria-hidden="true">
          {Array.from({ length: 12 }, (_, i) => (
            <i key={i} />
          ))}
        </div>
        <h2>{t("grad.done.title")}</h2>
        <div className="chat-bubble">
          <span className="chat-name">{name}</span>
          {reply}
        </div>
        <p className="muted">{t("grad.chatted", { name })}</p>
        <button className="primary" onClick={() => openPath(project.path)}>
          {t("grad.openFolder")}
        </button>

        <div className="next-guide">
          <strong>{t("grad.next.title")}</strong>
          {installed.length > 0 ? (
            <>
              <p>{t("grad.editor.installed.desc", { name })}</p>
              <div className="editor-btns">
                {installed.map((e) => (
                  <button
                    key={e.id}
                    className="ghost"
                    disabled={opening !== null}
                    onClick={() => openInEditor(e.id)}
                  >
                    {opening === e.id
                      ? t("grad.editor.preparing")
                      : t("grad.editor.open", { editor: editorLabel(t, e.id) })}
                  </button>
                ))}
              </div>
            </>
          ) : (
            <>
              <p>{t("grad.editor.none.desc")}</p>
              <div className="editor-btns">
                {(editors.length > 0 ? editors : fallback).map((e) => (
                  <button
                    key={e.id}
                    className="ghost"
                    onClick={() => openUrl(e.url)}
                  >
                    {t("grad.editor.get", { editor: editorLabel(t, e.id) })}
                  </button>
                ))}
              </div>
              <button className="link recheck" onClick={refreshEditors}>
                {t("grad.editor.recheck")}
              </button>
            </>
          )}
          <p className="hint">{t("grad.reopen")}</p>
        </div>

        <div className="next-guide">
          <strong>{t("tips.title")}</strong>
          <UsageTips agent={agent} />
        </div>

        <button className="link home-link" onClick={onHome}>
          {t("home.title")} →
        </button>
      </div>
    );
  }

  if (running) {
    return (
      <div className="center">
        <h2>{t("grad.writing.title", { name })}</h2>
        <p className="muted">{t("grad.writing.desc")}</p>
      </div>
    );
  }

  return (
    <div className="center">
      <h2>{t("grad.last.title")}</h2>
      <p className="muted">{t("grad.last.desc", { name })}</p>
      {error ? (
        <DoctorCard
          error={error}
          onRetry={chat}
          retryLabel={t("grad.sayHi", { name })}
        />
      ) : (
        <button className="primary" onClick={chat}>
          {t("grad.sayHi", { name })}
        </button>
      )}
    </div>
  );
}

export default App;
