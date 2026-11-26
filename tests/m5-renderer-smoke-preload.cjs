// Optional before/after captures (see m5-renderer-smoke-runner.cjs): only with the variables set.
const smokeEnv = typeof process === "undefined" ? {} : process.env;
if (smokeEnv.M5_SMOKE_CAPTURE_DIR) window.m5SmokeCapture = name => require("electron").ipcRenderer.invoke("m5-smoke-capture", name);
if (smokeEnv.M5_SMOKE_THEME) { try { localStorage.setItem("elsecrew.theme.v1", smokeEnv.M5_SMOKE_THEME); } catch {} }
// Rendered contrast at each capture point (PR #10 review I4); main keeps the results across page loads.
if (typeof require === "function") {
  const { measureRenderedContrast } = require("./m5-rendered-contrast-probe.cjs");
  window.m5SmokeMeasureOnly = measureRenderedContrast;
  window.m5SmokeMeasure = async (screen, options) => require("electron").ipcRenderer.invoke("m5-smoke-contrast", await measureRenderedContrast(screen, options));
}
// Folder confirmations (ADE U2, Q7): main's ask, played by the smoke; answers are recorded.
{
  let folderListener = null;
  window.m5SmokeFolderAnswers = [];
  window.m5FolderConfirm = {
    onAsk: listener => { folderListener = listener; return () => { if (folderListener === listener) folderListener = null; }; },
    answer: async (nonce, yes) => { window.m5SmokeFolderAnswers.push({ nonce, yes }); },
  };
  window.m5SmokeAskFolder = kind => folderListener?.({ nonce: "smoke-nonce-" + kind, kind, name: "new-shop", path: "/work/new-shop" });
}
window.m5SmokeClaudeUsageReads = 0;
window.m5ClaudeUsage={read:async()=>{window.m5SmokeClaudeUsageReads++;return ({state:"available",usage:{windows:[{id:"primary",usedPercent:12,windowMinutes:300,resetsAt:null},{id:"secondary",usedPercent:27,windowMinutes:10080,resetsAt:null}]},checkedAt:Date.now()});}};
let smokeConnections = {codex:false,"claude-code":false};
window.m5SmokeModelQueries = [];
window.m5Models = { request: async input => {
  if (input.chatId || input.roleId) throw new Error("catalog must not require a chat");
  window.m5SmokeModelQueries.push(input.runtime);
  return { schemaVersion:1, runtime:input.runtime, scope:null, usage: input.runtime === "codex" ? {windows:[{id:"primary",usedPercent:31,windowMinutes:300,resetsAt:2000000000}]} : null, models:[
    {modelSelector:"test-model", reasoningEfforts:["medium"]},
    {modelSelector:"new-default", reasoningEfforts:["medium","high"], isDefault:true},
  ] };
} };
window.m5AppUpdate = { open: async () => {}, onRestartCheck: (check, resume) => {
  window.m5SmokeUpdateCheck = check; window.m5SmokeUpdateResume = resume;
  return () => { window.m5SmokeUpdateCheck = undefined; window.m5SmokeUpdateResume = undefined; };
} };
const listeners = new Set();
window.m5SmokeAdmissionReturned = false;
window.m5SmokePrematureInstallReads = 0;
window.m5Install = { request: async () => {
  if (!window.m5SmokeAdmissionReturned) window.m5SmokePrematureInstallReads++;
  return {state:"unsupported", operationId:null, provider:null, plan:null, error:null, cancelRequested:false};
} };
const preparationInspection = { projectId: "earlier_project", worktreeId: "worktree_failed", workspaceIdentity: "workspace_failed", lockId: "lock_worktree_failed", projectRevision: 8, lockRevision: 9 };
let preparationAttempts = 0;
window.m5SmokePermissions = {};
window.m5CleanupCalls = [];
window.m5Projects = { request: async (input) => {
  if (input.action === "chatErasureList") return {schemaVersion:1,action:input.action,entries:[]};
  if (input.action === "workspaceCleanupList") return {schemaVersion:1,action:input.action,entries:[{chatId:"chat_cleanup",projectId:"smoke_project",title:"Discarded sample",eligible:true,pending:false},{chatId:"chat_blocked",projectId:"smoke_project",title:"Recovery required",eligible:false,pending:false}].filter(item=>!window.m5CleanupCalls.includes(item.chatId))};
  if (input.action === "cleanupWorkspace") {
    if (!input.confirmed || input.chatId !== "chat_cleanup") throw new Error("cleanup blocked");
    window.m5CleanupCalls.push(input.chatId);
    return {schemaVersion:1,action:input.action,chatId:input.chatId,deleted:true};
  }

  if(input.action === "getAutomationPermissions" || input.action === "setAutomationPermissions") {
    if(input.action === "setAutomationPermissions") {
      if(input.confirmed !== true) throw new Error("missing consent");
      window.m5SmokePermissions[input.projectId] = {...input.permissions};
    }
    return {schemaVersion:1,action:input.action,projectId:input.projectId,permissions:window.m5SmokePermissions[input.projectId] || {files:false,commands:false},recorded:Boolean(window.m5SmokePermissions[input.projectId])};
  }

  if(input.action === "getProviderConnections" || input.action === "setProviderConnection") {
    if(input.action === "setProviderConnection") smokeConnections={...smokeConnections,[input.runtime]:input.connected};
    return {schemaVersion:1,action:input.action,connections:{...smokeConnections}};
  }
  if (input.action === "getAppRoleDefaults" && window.m5SmokeDefaultsReady) return {
    schemaVersion:1, action:input.action, settings:{schemaVersion:1, revision:window.m5SmokeDefaultsRevision || 2,
      roles:smokeChat.roleSnapshots.filter(role => ["planning","coding","review"].includes(role.roleId)), updatedAt:smokeChat.updatedAt},
  };
  if (input.action === "cancelTeamTask") {
    for (const key of ["chatId", "taskId", "projectId", "worktreeId", "workspaceIdentity"]) if (input[key] !== smokeChat[key]) throw new Error("wrong team cancellation identity");
    if (input.triggerMessageId !== [...smokeMessages].reverse().find(m => m.kind === "user").messageId) throw new Error("wrong team task");
    window.m5SmokeTeamStops.push(input); smokeChat.state = "canceled";
    return {schemaVersion:1, action:input.action, chatId:input.chatId, triggerMessageId:input.triggerMessageId, status:"stopped"};
  }

  if (input.action === "inspectPreparation" && input.projectId === "earlier_project") return {schemaVersion: 1, action: input.action, inspection: preparationInspection};
  if (input.action === "abandonPreparation") {
    if (JSON.stringify(input.inspection) !== JSON.stringify(preparationInspection)) throw new Error("wrong preparation identity");
    if (++preparationAttempts === 1) throw new Error("synthetic response loss");
    return {schemaVersion: 1, action: input.action, inspection: preparationInspection};
  }
  if (input.action === "finishDeletions") return {schemaVersion: 1, action: input.action, pending: 0};
  if (input.action === "releaseView" && input.projectId === "smoke_project") return {schemaVersion: 1, action: "releaseView", projectId: input.projectId};
  if (input.action === "list" && window.m5SmokeCatalogEmpty) return {schemaVersion:1, action:"list", projects:[], nextCursor:null};
  if (input.action === "list") return input.cursor === null
    // After pagination is checked, the first page also lists the open project so
    // its rail ⋯ menu (where closing lives) is reachable while the rail is locked.
    ? {schemaVersion: 1, action: "list", projects: [{projectId: "earlier_project", canonicalPath: "/work/earlier"}, ...(window.m5SmokeCatalogListsCurrent ? [{projectId: "smoke_project", canonicalPath: "/work/smoke"}] : [])], nextCursor: window.m5SmokeCatalogListsCurrent ? "smoke_project" : "earlier_project"}
    : {schemaVersion: 1, action: "list", projects: [{projectId: "smoke_project", canonicalPath: "/work/smoke"}], nextCursor: null};
  if (input.action !== "reopen" || input.projectId !== "smoke_project") throw new Error("unexpected catalog selection");
  return {schemaVersion: 1, action: "reopen", project: {schemaVersion: 1, projectId: "smoke_project", selectedPath: "/work/smoke", canonicalGitRoot: "/work/smoke"}};
} };
const loginStates = {codex:{state: "idle", attemptId: null, authUrl: null, error: null},claude:{state: "idle", attemptId: null, authUrl: null, error: null}};
let codexRecoveryAttempts = 0;
let codexChecks = 0;
const diagnosedProviders = new Set();
window.m5SmokeLoginRequests = [];
window.m5SmokeLoginStatusFailure = false;
window.m5Login = { request: async (input) => {
  const provider = input.provider ?? "codex";
  window.m5SmokeLoginRequests.push({provider, action: input.action});
  if (provider === "codex" && ["status", "check"].includes(input.action) && window.m5SmokeLoginStatusFailure) throw new Error("synthetic status unavailable");
  let status = loginStates[provider];
  if (input.action === "diagnose") {
    if (status.error === "providerLoginRecoveryRequired") throw new Error("diagnosis cannot clear recovery");
    diagnosedProviders.add(provider);
    status = {...status,state:"installed",attemptId:input.attemptId,error:null};
  }
  if (input.action === "check") {
    if (!diagnosedProviders.has(provider)) throw new Error("account check before diagnosis");
    if (provider === "codex") codexChecks += 1;
    const recoveryRequired = provider === "codex" && codexChecks > 1 && codexRecoveryAttempts < 2;
    status = {...status,state:provider === "codex" ? (recoveryRequired ? "failed" : "connected") : "idle",attemptId:input.attemptId,error:recoveryRequired ? "providerLoginRecoveryRequired" : null};
  }
  if (input.action === "recover") {
    if (provider !== "codex" || status.error !== "providerLoginRecoveryRequired") throw new Error("unexpected recovery");
    codexRecoveryAttempts += 1;
    // First attempt has insufficient evidence. A later explicit attempt succeeds.
    await new Promise(resolve => setTimeout(resolve, 150));
    status = {...status,state:codexRecoveryAttempts < 2 ? "failed" : "idle",attemptId:input.attemptId,error:codexRecoveryAttempts < 2 ? "providerLoginRecoveryRequired" : null};
  }
  if (input.action === "start") {
    if (provider === "codex") throw new Error("restoration must not start OAuth");
    status = {...status, state: "waiting", attemptId: input.attemptId};
  }
  if (input.action === "cancel") status = {...status, state: "canceled"};
  if (input.action === "submitCode") {
    if (provider !== "claude" || status.state !== "waiting" || input.attemptId !== status.attemptId || input.code !== "synthetic#state") throw new Error("invalid code handoff");
    status = {...status,state:"connected"};
  }
  loginStates[provider] = status;
  return status;
} };
const workspaceListeners = new Set();
const chatListeners = new Set();
const agentPublicEventListeners = new Set();
const agentRunStateListeners = new Set();
let snapshotCreated = false;
let statusReads = 0;
const smokeChat = { schemaVersion: 2, chatId: "smoke_chat", taskId: "smoke_task", projectId: "smoke_project", worktreeId: "smoke_chat_worktree", workspaceIdentity: "smoke_chat_identity", goal: "후속 대화 확인", settingsRevision: 1, roleSnapshots: [
  {roleId:"planning",runtime:"codex",modelSelector:"test-model",reasoningEffort:"medium",permission:"readOnly"},
  {roleId:"coding",runtime:"claude-code",modelSelector:"test-model",reasoningEffort:null,permission:"workspaceWrite"},
  {roleId:"review",runtime:"codex",modelSelector:"test-model",reasoningEffort:null,permission:"readOnly"},
], state: "draft", createdAt: "2026-09-14T00:00:00Z", updatedAt: "2026-09-14T00:00:00Z" };
smokeChat.roleSnapshots = [{...smokeChat.roleSnapshots[0], roleId:'coordinator'}, smokeChat.roleSnapshots[0], {...smokeChat.roleSnapshots[0], roleId:'design'}, ...smokeChat.roleSnapshots.slice(1)];
window.m5SmokeContextStarts = [];
window.m5SmokeShortStarts = [];
const smokeMessages = [{ schemaVersion: 1, messageId: "smoke_initial", chatId: smokeChat.chatId, taskId: smokeChat.taskId, worktreeId: smokeChat.worktreeId, eventSequence: 1, kind: "user", roleId: null, runId: null, text: "처음 요청", createdAt: smokeChat.createdAt }];
window.m5SmokeTeamStops = [];
window.m5SmokeTeamMessages = () => {
  for (const [roleId, text] of [["coordinator", JSON.stringify({action:"delegate",role:"coding",instruction:"Inspect files",summary:"개발 담당과 구조를 확인할게요."})], ["coding", "파일 구조를 확인했습니다."], ["coordinator", JSON.stringify({action:"final",answer:"확인한 내용을 정리해 드릴게요."})]]) {
    const eventSequence = smokeMessages.at(-1).eventSequence + 1;
    const message = {...smokeMessages[0],kind:"role",roleId,runId:"smoke_team_"+eventSequence,messageId:"team_message_"+eventSequence,eventSequence,text};
    smokeMessages.push(message); for (const listener of chatListeners) listener(message);
  }
};
let historyMessageIndex = 0;
let historyReadFailures = 0;
let diagnosticRun = null;
let diagnosticReads = 0;
window.m5Diagnostics = {inspect: async () => {
  diagnosticReads++;
  if (diagnosticReads === 2) throw new Error('synthetic diagnostic failure');
  return {scope:'currentAppSession',claudeCheckpoints:diagnosticReads === 1 ? ['init model'] : []};
}};
window.m5SmokeFailedClaude = () => {
  diagnosticRun = {schemaVersion:1,runId:diagnosticRun?.runId ?? 'smoke_failed_claude',chatId:smokeChat.chatId,
    taskId:smokeChat.taskId,projectId:smokeChat.projectId,worktreeId:smokeChat.worktreeId,
    workspaceIdentity:smokeChat.workspaceIdentity,selectedProfile:smokeChat.roleSnapshots[3],
    source:'actualAgent',capabilityFingerprint:`sha256:${'0'.repeat(64)}`,state:'failed',
    stage:'implementing',terminationOutcome:'cleanExit',createdAt:smokeChat.createdAt,updatedAt:smokeChat.updatedAt};
  for(const listener of agentRunStateListeners) listener({run:diagnosticRun});
};
window.m5SmokeVisualRunning = () => {
  diagnosticRun = {schemaVersion:1,runId:'agent_run_visual_running',chatId:smokeChat.chatId,taskId:smokeChat.taskId,
    projectId:smokeChat.projectId,worktreeId:smokeChat.worktreeId,workspaceIdentity:smokeChat.workspaceIdentity,
    selectedProfile:smokeChat.roleSnapshots[3],source:'actualAgent',capabilityFingerprint:`sha256:${'0'.repeat(64)}`,
    state:'running',stage:'implementing',terminationOutcome:null,createdAt:smokeChat.createdAt,updatedAt:smokeChat.updatedAt};
  for (const listener of agentRunStateListeners) listener({run:diagnosticRun});
};
// Visual-only state transition; no process or provider request is performed.
window.m5SmokeVisualChatState = state => {
  if (!['draft', 'running', 'reviewReady', 'recoveryRequired'].includes(state)) throw new Error('unexpected visual state');
  smokeChat.state = state;
  for (const listener of agentPublicEventListeners) listener({ ...smokeChat, runId:'smoke_failed_claude', roleId:'coding', eventId:'visual_state_' + state, kind:'status', text:'visual state', capabilityFingerprint:`sha256:${'0'.repeat(64)}` });
};
window.m5SmokeFailHistoryRefresh = () => {
  historyReadFailures = 1;
  for (const listener of agentPublicEventListeners) listener({ ...smokeChat, runId:"smoke_context_run", roleId:"planning", eventId:"smoke_refresh_event", kind:"status", text:"not durable", capabilityFingerprint:`sha256:${"0".repeat(64)}` });
};
let parallelPaint = null;
window.m5SmokeEnableParallelChats = () => {
  if (parallelPaint) throw new Error("parallel paint fixture already enabled");
  const { parallelChatFixture } = require("./m5-parallel-chat-fixture.cjs");
  parallelPaint = parallelChatFixture(smokeChat, message => { for (const listener of chatListeners) listener(message); });
  return parallelPaint.chats;
};
// The tree has no root button at the top level any more; an overflowing watch
// is the real path that offers a full reload ("다시 확인").
const SMOKE_READ_VERSION = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const SMOKE_DIFF_ENTRIES = [{ path: "src/example.ts", kind: "modified", binary: false }];
function smokeFileText(path) {
  return path.endsWith(".rs") ? "fn main() {}\n" : path.endsWith(".py") ? "def main():\n    pass\n" : path.endsWith(".json") ? '{"ready":true}\n' : "export const smoke = true;\n";
}
function smokeDiffText() {
  return "@@ -1 +1,401 @@\n-old\n" + Array.from({ length: 401 }, (_, index) => `+line ${index + 1}\n`).join("");
}
function workspaceRef(input) {
  return { chatId: input.chatId, projectId: input.projectId, worktreeId: input.worktreeId, workspaceIdentity: input.workspaceIdentity };
}
function emitWorkspaceChunk(input, streamId, text) {
  for (const listener of workspaceListeners) listener({ protocolVersion: 1, kind: "event", sidecarInstanceId: "smoke_sidecar", streamId, seq: 1, event: "workspace.blob.chunk", payload: { ...workspaceRef(input), text } });
}
function smokeTreePage(parentPath, cursor) {
    if (window.m5SmokeLongTree && parentPath === "") {
      window.m5SmokeTreeReads = (window.m5SmokeTreeReads ?? 0) + 1;
      return { projectId: "smoke_project", parentPath, entries: Array.from({length: cursor ? 1 : 500}, (_, i) => {
        const name = `file-${String((cursor ? 500 : 0) + i).padStart(4, "0")}.ts`;
        return {path:name,name,kind:"file"};
      }), nextCursor: cursor ? null : "next-tree" };
    }
    return {
      projectId: "smoke_project",
      parentPath,
      entries: parentPath === "src/nested" ? [{path:"src/nested/child.ts",name:"child.ts",kind:"file"}] : parentPath === "src" ? [{path:"src/nested",name:"nested",kind:"directory"}, ...["example.ts", "sample.rs", "sample.py", "sample.json"].map(name => ({ path: `src/${name}`, name, kind: "file" }))] : [...(window.m5SmokeHiddenTree ? [{path:".git",name:".git",kind:"directory"}] : []), { path: "src", name: "src", kind: "directory" }],
      nextCursor: null,
    };
}
const projectListeners = new Set();
window.m5SmokeRescan = () => {
  for (const listener of projectListeners) listener({ projectId: "smoke_project", event: "file.changed", paths: [], rescanRequired: true });
  // The resumed task's workspace watches too (opening a project resumes its last task).
  for (const listener of workspaceListeners) listener({ protocolVersion: 1, kind: "event", sidecarInstanceId: "smoke", streamId: "smoke_watch", seq: 1, event: "workspace.changed",
    payload: { chatId: smokeChat.chatId, projectId: smokeChat.projectId, worktreeId: smokeChat.worktreeId, workspaceIdentity: smokeChat.workspaceIdentity, event: "file.changed", paths: [], rescanRequired: true } });
};
window.m5SmokeEmitParallelRound = () => {
  if (!parallelPaint) throw new Error("parallel paint fixture not enabled");
  return parallelPaint.emitRound();
};
window.m5SmokeParallelCounts = () => parallelPaint?.counts() ?? null;
window.m5SmokeAppendMessages = (count) => {
  if (!Number.isSafeInteger(count) || count < 1 || count > 5000) throw new Error("invalid synthetic message count");
  const emitted = [];
  for (let index = 0; index < count; index++) {
    const eventSequence = smokeMessages[smokeMessages.length - 1].eventSequence + 1;
    const message = { ...smokeMessages[0], messageId: `history_${++historyMessageIndex}`, eventSequence, text: `history message ${historyMessageIndex}` };
    smokeMessages.push(message);
    emitted.push(message.text);
    for (const listener of chatListeners) listener(message);
  }
  return emitted;
};

const unexpected = (method) => async () => { throw new Error(`renderer smoke fixture did not expect ${method}`); };
const unsubscribe = (listeners, listener) => {
  listeners.add(listener);
  return () => listeners.delete(listener);
};

window.m5 = {
  version: 7,
  appendChatMessage: async (request) => {
    const message = { ...smokeMessages[0], messageId: request.clientMessageId, text: request.text, eventSequence: request.expectedEventSequence + 1 };
    smokeMessages.push(message);
    for (const listener of chatListeners) listener(message);
    return message;
  },
  getSystemStatus: async function () {
    if (this !== window.m5) throw new TypeError("Illegal invocation");
    // Browser fixture: leave enough time for mount effects before admission.
    // The isolated contract VM has no timer and does not mount React.
    if (!window.m5SmokeAdmissionReturned && typeof setTimeout === 'function') await new Promise(resolve => setTimeout(resolve, 100));
    window.m5SmokeAdmissionReturned = true;
    const status = {
    schemaVersion: 1,
    state: "shellReady",
    sidecar: {
      protocolVersion: 1,
      sidecarSchemaVersion: 1,
      appSchemaVersion: 1,
      pid: 42,
      platform: "darwin",
      capabilities: [
        "system.status", "app.prepareQuit", "project.open", "project.getTree", "project.createWorktree", "project.close",
        "file.read", "file.save", "file.watch", "diff.getSummary", "diff.getFile", "workspace.getTree", "workspace.close",
        "workspace.file.read", "workspace.file.save", "workspace.file.watch", "workspace.diff.getSummary", "workspace.diff.getFile",
        "workspace.prepareChat", "workspace.abandonPrepared", "project.getRoleSettings", "project.updateRoleSettings", "chat.create", "chat.list", "chat.get",
        "run.startFixture", "run.cancel", "recovery.list", "recovery.inspect", "recovery.markResolved", "storage.status",
      ],
      sidecarInstanceId: "smoke_sidecar",
    },
    storageAdmission: {
      schemaVersion: 1,
      state: "ready",
      startedAt: "2026-09-09T00:00:00Z",
      completedAt: "2026-09-09T00:00:01Z",
      error: null,
    },
    database: { health: "healthy", schemaVersion: 1 },
    recoveryRequired: (window.m5SmokeBlockers ?? ["lock:before_chat"]).length > 0,
    blockerIds: window.m5SmokeBlockers ?? ["lock:before_chat"],
    };
    if (++statusReads === 1) {
      status.storageAdmission.state = "verifying";
      status.storageAdmission.completedAt = null;
      status.database = {health: "verifying", schemaVersion: null};
      status.recoveryRequired = null;
      status.blockerIds = [];
    }
    return status;
  },
  getStorageStatus: unexpected("getStorageStatus"),
  prepareQuit: async () => ({ safeToQuit: true, blockers: [] }),
  onSidecarEvent: (listener) => unsubscribe(listeners, listener),
  // ADE U2·U3 smoke: a cancelled folder pick, and a project with no task yet (first-task window).
  openProject: async () => window.m5SmokeOpenCancel ? null : ({
    schemaVersion: 1,
    projectId: "smoke_project",
    selectedPath: "/work/smoke",
    canonicalGitRoot: "/work/smoke",
  }),
  getProjectTree: async ({ parentPath = "", cursor = null }) => {
    if (!snapshotCreated) throw new Error("renderer requested the tree before creating its standalone snapshot");
    return smokeTreePage(parentPath, cursor);
  },
  createProjectWorktree: async ({ projectId }) => {
    if (projectId !== "smoke_project") throw new Error("renderer supplied an unexpected project ID");
    snapshotCreated = true;
    return {
      schemaVersion: 1,
      projectId,
      canonicalGitRoot: "/work/smoke",
      worktreeId: "worktree_smoke",
      worktreePath: "/private/workspaces/worktree_smoke",
      workspaceKind: "standaloneSnapshot",
      snapshotTransport: "gitBundle",
      sourceMutationPolicy: "forbidden",
      objectFormat: "sha1",
      baseCommit: "a".repeat(40),
      baseTree: "b".repeat(40),
      sourceIdentity: "source_smoke",
      workspaceIdentity: "workspace_smoke",
      headMode: "detached",
      lifecycle: "active",
    };
  },
  closeProject: async () => ({ projectId: "smoke_project", lifecycle: "abandoned", physicallyDeleted: false }),
  readFile: async ({ path }) => {
    const text = smokeFileText(path);
    for (const listener of listeners) {
      listener({
        protocolVersion: 1,
        kind: "event",
        sidecarInstanceId: "smoke_sidecar",
        streamId: "smoke_file",
        seq: 1,
        event: "blob.chunk",
        payload: { text },
      });
    }
    return {
      projectId: "smoke_project",
      path,
      classification: "utf8",
      byteLength: Buffer.byteLength(text),
      version: "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
      stream: { streamId: "smoke_file", chunkBytes: 262144 },
    };
  },
  beginFileSave: async () => ({ saveId: "smoke_save", chunkBytes: 262144 }),
  appendFileSaveChunk: async ({ seq }) => ({ acceptedSeq: seq }),
  commitFileSave: async () => ({ outcome: "committed", version: "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb" }),
  watchProjectFiles: async () => {
    if (!snapshotCreated) throw new Error("renderer watched before creating its standalone snapshot");
    return { watching: true };
  },
  getDiffSummary: async () => {
    if (!snapshotCreated) throw new Error("renderer requested changes before creating its standalone snapshot");
    return { projectId: "smoke_project", baseCommit: "a".repeat(40), entries: SMOKE_DIFF_ENTRIES };
  },
  getDiffFile: async () => {
    const text = smokeDiffText();
    for (const listener of listeners) listener({ protocolVersion: 1, kind: "event", sidecarInstanceId: "smoke_sidecar", streamId: "smoke_diff", seq: 1, event: "blob.chunk", payload: { text } });
    return { projectId: "smoke_project", path: "src/example.ts", kind: "modified", oldPath: null, baseVersion: "sha256:" + "a".repeat(64), worktreeVersion: "sha256:" + "b".repeat(64), byteLength: text.length, stream: { streamId: "smoke_diff", chunkBytes: 262144 } };
  },
  onProjectEvent: (listener) => unsubscribe(projectListeners, listener),
  // Opening the project resumes its last task (2026-11-17), so the tree the
  // screen shows first is that task's workspace: the same smoke files.
  getWorkspaceTree: async (input) => {
    const page = smokeTreePage(input.parentPath ?? "", input.cursor ?? null);
    return { ...input, entries: page.entries, nextCursor: page.nextCursor };
  },
  // Opening a project resumes its last task, so files, saves and comparisons
  // go through the task's workspace. They serve the same smoke content as the
  // project calls, with the workspace address and `workspace.blob.chunk`
  // events; a save commits through `commitFileSave`, so wrappers the runner
  // puts on it (save counting) still see every save.
  closeWorkspace: async (input) => ({ ...workspaceRef(input), lifecycle: "abandoned", physicallyDeleted: false }),
  readWorkspaceFile: async (input) => {
    const text = smokeFileText(input.path);
    emitWorkspaceChunk(input, "smoke_workspace_file", text);
    return { ...workspaceRef(input), path: input.path, classification: "utf8", byteLength: Buffer.byteLength(text), version: SMOKE_READ_VERSION, stream: { streamId: "smoke_workspace_file", chunkBytes: 262144 } };
  },
  beginWorkspaceFileSave: async (input) => ({ ...workspaceRef(input), saveId: "smoke_workspace_save", chunkBytes: 262144 }),
  appendWorkspaceFileSaveChunk: async (input) => ({ ...workspaceRef(input), saveId: input.saveId, acceptedSeq: input.seq }),
  commitWorkspaceFileSave: async (input) => ({ ...(await window.m5.commitFileSave(input)), ...workspaceRef(input), saveId: input.saveId }),
  watchWorkspaceFiles: async (input) => ({ ...input, watching: true }),
  getWorkspaceDiffSummary: async (input) => ({ ...workspaceRef(input), baseCommit: "a".repeat(40), entries: SMOKE_DIFF_ENTRIES }),
  getWorkspaceDiffFile: async (input) => {
    const text = smokeDiffText();
    emitWorkspaceChunk(input, "smoke_workspace_diff", text);
    return { ...workspaceRef(input), path: input.path, kind: "modified", oldPath: null, baseVersion: "sha256:" + "a".repeat(64), worktreeVersion: "sha256:" + "b".repeat(64), byteLength: text.length, stream: { streamId: "smoke_workspace_diff", chunkBytes: 262144 } };
  },
  onWorkspaceEvent: (listener) => unsubscribe(workspaceListeners, listener),
  prepareChatWorkspace: unexpected("prepareChatWorkspace"),
  abandonPreparedWorkspace: unexpected("abandonPreparedWorkspace"),
  getProjectRoleSettings: async ({ projectId }) => ({
    schemaVersion: 1,
    projectId,
    revision: 1,
    roles: [
      { roleId: "planning", runtime: "codex", modelSelector: "test-model", reasoningEffort: "medium", permission: "readOnly" },
      { roleId: "coding", runtime: "claude-code", modelSelector: "test-model", reasoningEffort: null, permission: "workspaceWrite" },
      { roleId: "review", runtime: "codex", modelSelector: "test-model", reasoningEffort: null, permission: "readOnly" },
    ],
    updatedAt: "2026-08-26T00:00:00Z",
  }),
  updateProjectRoleSettings: unexpected("updateProjectRoleSettings"),
  createChat: unexpected("createChat"),
  listChats: async ({ projectId }) => ({ projectId, items: window.m5SmokeNoChats ? [] : parallelPaint ? parallelPaint.chats : [smokeChat], nextCursor: null }),
  getChat: async (request) => {
    if (historyReadFailures > 0) { historyReadFailures--; throw new Error("synthetic history read failure"); }
    return parallelPaint ? parallelPaint.detail(request) : ({ chat: smokeChat, afterEventSequence: request.afterEventSequence, messages: smokeMessages.filter(message => message.eventSequence > request.afterEventSequence), runs: [], agentRuns:diagnosticRun ? [diagnosticRun] : [], agentRecovery:null, nextEventSequence: null });
  },
  startFixtureRun: unexpected("startFixtureRun"),
  cancelRun: unexpected("cancelRun"),
  listRecoveries: async () => ({ items: [], nextCursor: null }),
  inspectRecovery: unexpected("inspectRecovery"),
  markRecoveryResolved: unexpected("markRecoveryResolved"),
  onChatEvent: (listener) => unsubscribe(chatListeners, listener),
  getAgentCapabilities: async () => ({canStart:true, blockedReason:null, fingerprint:`sha256:${"0".repeat(64)}`}),
  startAgentRun: async (request) => {
    if (smokeMessages.length <= 100) {
      if (request.roleId !== "coordinator") throw new Error("expected coordinator");
      window.m5SmokeShortStarts.push(request);
      return {runId:"smoke_short_run"};
    }
    const expected = [smokeMessages[0].messageId, ...smokeMessages.slice(-99).map(message => message.messageId)];
    if (smokeMessages.length <= 100 || request.chatId !== smokeChat.chatId || request.roleId !== "coordinator"
        || JSON.stringify(request.inputMessageIds) !== JSON.stringify(expected)) throw new Error("unexpected long-chat input");
    window.m5SmokeContextStarts.push(request);
    return {schemaVersion:1,runId:"smoke_context_run",chatId:smokeChat.chatId,taskId:smokeChat.taskId,
      projectId:smokeChat.projectId,worktreeId:smokeChat.worktreeId,workspaceIdentity:smokeChat.workspaceIdentity,
      roleId:request.roleId,capabilityFingerprint:request.expectedCapabilityFingerprint,state:"preparing"};
  },
  cancelAgentRun: unexpected("cancelAgentRun"),
  inspectAgentRecovery: unexpected("inspectAgentRecovery"),
  resolveAgentRecovery: unexpected("resolveAgentRecovery"),
  onAgentPublicEvent: (listener) => unsubscribe(agentPublicEventListeners, listener),
  onAgentRunStateChanged: (listener) => unsubscribe(agentRunStateListeners, listener),
};
