import { useEffect, useMemo, useRef, useSyncExternalStore, type ReactNode } from "react";
import type * as Monaco from "monaco-editor/esm/vs/editor/editor.api";

import {
  ProjectEditorController,
  type EditorTab,
  type WorkbenchPanel,
} from "./editor-state.ts";
import type { DiffEntryV1 } from "./contracts/diff-v1.ts";

type MonacoApi = typeof Monaco;

let monacoLoader: Promise<MonacoApi> | undefined;

function loadMonaco(): Promise<MonacoApi> {
  monacoLoader ??= Promise.all([
    import("monaco-editor/esm/vs/editor/editor.api"),
    import("monaco-editor/esm/vs/editor/editor.worker?worker"),
    import("monaco-editor/esm/vs/basic-languages/markdown/markdown.contribution"),
    import("monaco-editor/esm/vs/basic-languages/typescript/typescript.contribution"),
  ]).then(([api, editorWorker]) => {
    // Vite emits the worker alongside the local renderer assets; no CDN or eval loader is used.
    (globalThis as typeof globalThis & { MonacoEnvironment?: Monaco.Environment }).MonacoEnvironment = {
      getWorker: () => new editorWorker.default(),
    };
    return api;
  });
  return monacoLoader;
}

export function M5Workbench({ bridge = window.m5 }: { bridge?: typeof window.m5 }) {
  const controller = useMemo(() => new ProjectEditorController(bridge), [bridge]);
  const snapshot = useSyncExternalStore(
    (listener) => controller.subscribe(listener),
    () => controller.snapshot(),
    () => controller.snapshot(),
  );
  useEffect(() => {
    controller.start();
    return () => controller.dispose();
  }, [controller]);

  const activeTab = snapshot.activePath
    ? snapshot.tabs.find((tab) => tab.path === snapshot.activePath) ?? null
    : null;

  return (
    <main className="m5-workbench" aria-label="M5 코딩 작업대">
      <header className="m5-toolbar">
        <div>
          <p className="m5-eyebrow">M5 · 원본과 분리된 작업 복사본</p>
          <h1>{snapshot.project?.name ?? "프로젝트를 선택해 시작하세요"}</h1>
          <p className="m5-subtitle">
            {snapshot.project ? "원본 프로젝트와 분리된 복사본에서 파일과 변경 내용을 안전하게 확인하고 있어요." : "Git 프로젝트를 고르면 원본과 분리된 작업 복사본의 파일과 변경 내용을 안전하게 확인할 수 있어요."}
          </p>
        </div>
        <div className="m5-toolbar-actions">
          <button type="button" onClick={() => void controller.chooseProject()} disabled={!controller.canLeaveProject() || snapshot.selectionBlocked}>
            {snapshot.project ? "프로젝트 바꾸기" : "프로젝트 선택"}
          </button>
          {snapshot.project && (
            <button className="m5-secondary" type="button" onClick={() => void controller.abandonProject()} disabled={!controller.canLeaveProject()}>
              작업 복사본 닫기
            </button>
          )}
        </div>
      </header>

      {snapshot.notice && <p className="m5-notice" role="status">{snapshot.notice}</p>}
      {snapshot.error && (
        <section className="m5-error" role="alert">
          <strong>{snapshot.error.message}</strong>
          <details><summary>자세히 보기</summary><pre>{snapshot.error.technicalDetail}</pre></details>
        </section>
      )}
      {snapshot.rescanRequired && (
        <section className="m5-rescan" role="status">
          <span>파일 변경이 많아 목록이 최신이 아닐 수 있어요.</span>
          <button type="button" onClick={() => void Promise.all([controller.loadTree(), controller.refreshChanges()])}>다시 확인</button>
        </section>
      )}

      <section className="m5-panels" aria-label="파일, 편집기, 변경 패널">
        <aside className="m5-panel m5-files" aria-label="파일">
          <PanelTitle active={snapshot.panel === "files"} panel="files" onSelect={controller.selectPanel.bind(controller)}>
            파일
          </PanelTitle>
          {snapshot.project ? (
            <>
              <button className="m5-root" type="button" title={snapshot.project.root} onClick={() => void controller.loadTree("")}>{snapshot.treeParent || snapshot.project.root}</button>
              <div className="m5-file-list">
                {snapshot.tree.map((entry) => (
                  <button key={entry.path} type="button" className="m5-file-row" onClick={() => entry.kind === "file" ? void controller.openFile(entry.path) : entry.kind === "directory" ? void controller.loadTree(entry.path) : undefined} disabled={entry.kind === "symlink"}>
                    <span aria-hidden="true">{entry.kind === "directory" ? "▸" : entry.kind === "symlink" ? "↗" : "·"}</span>
                    {entry.name}
                  </button>
                ))}
              </div>
            </>
          ) : <Empty text="프로젝트를 고르면 파일을 표시해요." />}
        </aside>

        <section className="m5-panel m5-editor" aria-label="코드 편집기">
          <div className="m5-tab-list" role="tablist" aria-label="열린 파일">
            {snapshot.tabs.map((tab) => (
              <div className="m5-editor-tab" key={tab.path}>
                <button type="button" role="tab" aria-selected={snapshot.activePath === tab.path} onClick={() => controller.openFile(tab.path)}>
                  {fileName(tab.path)} {tab.saveState !== "clean" && <span aria-label="저장되지 않은 변경">●</span>}
                </button>
                <button type="button" className="m5-close-tab" aria-label={`${tab.path} 탭 닫기`} onClick={() => controller.closeTab(tab.path)}>×</button>
              </div>
            ))}
          </div>
          <div className="m5-editor-head">
            <span>{activeTab?.path ?? "파일을 선택하세요"}</span>
            {activeTab?.mode === "text" && (
              <button type="button" onClick={() => void controller.saveActive()} disabled={activeTab.saveState !== "dirty"}>
                {activeTab.saveState === "saving" ? "저장 중…" : activeTab.saveState === "conflicted" ? "충돌 해결 필요" : "저장"}
              </button>
            )}
          </div>
          <EditorContent tab={activeTab} onChange={(text) => activeTab && controller.updateText(activeTab.path, text)} />
        </section>

        <aside className="m5-panel m5-changes" aria-label="변경">
          <PanelTitle active={snapshot.panel === "changes"} panel="changes" onSelect={controller.selectPanel.bind(controller)}>
            변경 {snapshot.diffSummary ? `(${snapshot.diffSummary.entries.length})` : ""}
          </PanelTitle>
          {snapshot.project ? (
            <>
              <button className="m5-refresh" type="button" onClick={() => void controller.refreshChanges()}>변경 새로 고침</button>
              <div className="m5-change-list">
                {snapshot.diffSummary?.entries.map((entry) => (
                  <button key={`${entry.kind}:${entry.path}`} type="button" className="m5-change-row" onClick={() => void controller.openDiff(entry.path)}>
                    <ChangeKind entry={entry} />
                    <span>{entry.path}</span>
                  </button>
                ))}
                {snapshot.diffSummary?.entries.length === 0 && <Empty text="아직 확인할 변경이 없어요." />}
              </div>
              {snapshot.diffFile && <DiffDetails descriptor={snapshot.diffFile} text={snapshot.diffText} />}
            </>
          ) : <Empty text="변경 내용은 프로젝트를 선택한 뒤 보여요." />}
        </aside>
      </section>
    </main>
  );
}

function PanelTitle({ active, panel, onSelect, children }: { active: boolean; panel: WorkbenchPanel; onSelect: (panel: WorkbenchPanel) => void; children: ReactNode }) {
  return <button type="button" className="m5-panel-title" aria-pressed={active} onClick={() => onSelect(panel)}>{children}</button>;
}

function EditorContent({ tab, onChange }: { tab: EditorTab | null; onChange: (text: string) => void }) {
  if (!tab) return <Empty text="파일을 열면 여기서 코드를 읽고 편집할 수 있어요." />;
  if (tab.mode === "binary") return <Empty text="이 파일은 이진 파일이라 내용을 편집하지 않아요." />;
  if (tab.mode === "tooLarge") return <Empty text="5MiB보다 큰 파일은 안전하게 미리 보지 않아요." />;
  if (tab.saveState === "conflicted") {
    return <section className="m5-conflict"><strong>저장 충돌</strong><p>내 초안은 그대로 보존했어요. 최신 파일을 확인한 뒤 새 변경으로 저장해 주세요.</p><details><summary>기술 정보</summary><pre>{tab.technicalDetail ?? "external file change"}</pre></details></section>;
  }
  return <MonacoEditor tab={tab} onChange={onChange} />;
}

function MonacoEditor({ tab, onChange }: { tab: EditorTab; onChange: (text: string) => void }) {
  const host = useRef<HTMLDivElement>(null);
  const editor = useRef<Monaco.editor.IStandaloneCodeEditor | null>(null);
  const model = useRef<Monaco.editor.ITextModel | null>(null);

  useEffect(() => {
    let cancelled = false;
    let cleanup: (() => void) | undefined;
    void loadMonaco().then((monaco) => {
      if (cancelled || !host.current) return;
      const nextModel = monaco.editor.createModel(tab.text, languageFor(tab.path));
      model.current = nextModel;
      const nextEditor = monaco.editor.create(host.current, {
        model: nextModel,
        automaticLayout: true,
        minimap: { enabled: false },
        fontSize: 13,
        readOnly: tab.saveState === "saving",
        ariaLabel: `${tab.path} 코드 편집기`,
      });
      editor.current = nextEditor;
      const subscription = nextModel.onDidChangeContent(() => onChange(nextModel.getValue()));
      cleanup = () => {
        subscription.dispose();
        nextEditor.dispose();
        nextModel.dispose();
        editor.current = null;
        model.current = null;
      };
    });
    return () => {
      cancelled = true;
      cleanup?.();
    };
  }, [tab.path]);

  useEffect(() => {
    const currentModel = model.current;
    if (currentModel && currentModel.getValue() !== tab.text) currentModel.setValue(tab.text);
    editor.current?.updateOptions({ readOnly: tab.saveState === "saving" });
  }, [tab.text, tab.saveState]);

  return <div className="m5-monaco" ref={host} />;
}

function DiffDetails({ descriptor, text }: { descriptor: NonNullable<ReturnType<ProjectEditorController["snapshot"]>["diffFile"]>; text: string }) {
  return (
    <section className="m5-diff-details" aria-label="선택한 변경 상세">
      <strong>{descriptor.path}</strong>
      <p>{labelForKind(descriptor.kind)}{descriptor.oldPath ? ` · 이전: ${descriptor.oldPath}` : ""}</p>
      {descriptor.kind === "binary" ? <Empty text="이진 변경은 내용 대신 파일 변경 사실만 보여요." /> : <pre>{text || "변경 내용을 준비하고 있어요."}</pre>}
    </section>
  );
}

function ChangeKind({ entry }: { entry: DiffEntryV1 }) {
  return <span className={`m5-change-kind m5-kind-${entry.kind}`}>{labelForKind(entry.kind)}</span>;
}

function labelForKind(kind: DiffEntryV1["kind"]): string {
  return ({ added: "추가", modified: "수정", deleted: "삭제", renamed: "이름 변경", binary: "이진" })[kind];
}

function languageFor(path: string): string {
  if (path.endsWith(".tsx") || path.endsWith(".ts")) return "typescript";
  if (path.endsWith(".js") || path.endsWith(".jsx")) return "javascript";
  if (path.endsWith(".json")) return "json";
  if (path.endsWith(".md")) return "markdown";
  return "plaintext";
}

function fileName(path: string): string {
  const parts = path.split("/");
  return parts[parts.length - 1] ?? path;
}

function Empty({ text }: { text: string }) {
  return <p className="m5-empty">{text}</p>;
}
