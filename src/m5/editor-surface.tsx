import { MarkdownView } from "./markdown-view.tsx";
import { memo, useEffect, useLayoutEffect, useMemo, useRef, useState, type ReactNode } from "react";
import type * as Monaco from "monaco-editor/esm/vs/editor/editor.api";
import { useI18n } from "../i18n";
import { ProjectEditorController, type EditorTab, type WorkbenchPanel } from "./editor-state.ts";
import type { TreeEntryV1 } from "./contracts/project-v1.ts";
import { diffPages, diffSides, splitDiffPage, DIFF_PAGE_ROWS } from "./diff-view.ts";
import { requestEditorSave } from "./editor-actions.ts";
import { languageFor } from "./editor-language.ts";
import { isProsePath } from "./editor-paths.ts";

type MonacoApi = typeof Monaco;

let monacoLoader: Promise<MonacoApi> | undefined;

function loadMonaco(): Promise<MonacoApi> {
  monacoLoader ??= Promise.all([
    import("monaco-editor/esm/vs/editor/editor.api"),
    import("monaco-editor/esm/vs/editor/editor.worker?worker"),
    import("monaco-editor/esm/vs/language/json/json.worker?worker"),
    import("monaco-editor/esm/vs/language/json/monaco.contribution"),
    import("monaco-editor/esm/vs/basic-languages/markdown/markdown.contribution"),
    import("monaco-editor/esm/vs/basic-languages/typescript/typescript.contribution"),
    import("monaco-editor/esm/vs/basic-languages/javascript/javascript.contribution"),
    import("monaco-editor/esm/vs/basic-languages/python/python.contribution"),
    import("monaco-editor/esm/vs/basic-languages/rust/rust.contribution"),
    import("monaco-editor/esm/vs/basic-languages/go/go.contribution"),
    import("monaco-editor/esm/vs/basic-languages/shell/shell.contribution"),
    import("monaco-editor/esm/vs/basic-languages/yaml/yaml.contribution"),
    import("monaco-editor/esm/vs/basic-languages/css/css.contribution"),
    import("monaco-editor/esm/vs/basic-languages/html/html.contribution"),
    import("monaco-editor/esm/vs/basic-languages/sql/sql.contribution"),
    import("monaco-editor/esm/vs/basic-languages/dockerfile/dockerfile.contribution"),
  ]).then(([api, editorWorker, jsonWorker]) => {
    // Monaco 0.52 JSON defaults disable remote schema requests. Keep local workers.
    // Vite emits the worker alongside the local renderer assets; no CDN or eval loader is used.
    (globalThis as typeof globalThis & { MonacoEnvironment?: Monaco.Environment }).MonacoEnvironment = {
      getWorker: (_id, label) => label === "json" ? new jsonWorker.default() : new editorWorker.default(),
    };
    return api;
  }).catch((error: unknown) => {
    monacoLoader = undefined;
    throw error;
  });
  return monacoLoader;
}

export const VirtualFileList = memo(function VirtualFileList({ showHidden, entries: rootEntries, readBranch, rootPage, activePath, disabled, onOpen, hasMore, loading, failed, onLoadMore }: { showHidden: boolean; entries: readonly TreeEntryV1[]; rootPage: number; readBranch: (path: string, cursor: string | null) => Promise<{entries: readonly TreeEntryV1[]; nextCursor: string | null}>; activePath: string | null; disabled: boolean; onOpen: (entry: TreeEntryV1) => void; hasMore: boolean; loading: boolean; failed: boolean; onLoadMore: () => void }) {
  type Branch = { entries: readonly TreeEntryV1[]; cursor: string | null; open: boolean; loading: boolean; failed: boolean };
  const [branches, setBranches] = useState<Record<string, Branch>>({});
  const branchesRef = useRef(branches);
  const epoch = useRef(0);
  const updateBranches = (next: Record<string, Branch>) => { branchesRef.current = next; setBranches(next); };
  useEffect(() => { if (rootPage === 1) { epoch.current++; updateBranches({}); } }, [rootEntries, rootPage]);
  useEffect(() => () => { epoch.current++; }, []);
  const loadBranch = async (path: string) => {
    const previous = branchesRef.current[path];
    if (disabled || previous?.loading) return;
    const generation = epoch.current;
    updateBranches({ ...branchesRef.current, [path]: { ...(previous ?? { entries: [], cursor: null, open: true }), loading: true, failed: false } });
    try {
      const page = await readBranch(path, previous?.cursor ?? null);
      if (generation !== epoch.current) return;
      updateBranches({ ...branchesRef.current, [path]: { ...branchesRef.current[path], entries: [...(previous?.entries ?? []), ...page.entries], cursor: page.nextCursor, loading: false, failed: false } });
    } catch {
      if (generation !== epoch.current) return;
      updateBranches({ ...branchesRef.current, [path]: { ...branchesRef.current[path], loading: false, failed: true } });
    }
  };
  const toggle = (entry: TreeEntryV1) => {
    if (entry.kind !== "directory") { onOpen(entry); return; }
    const branch = branchesRef.current[entry.path];
    if (!branch) { void loadBranch(entry.path); return; }
    updateBranches({ ...branchesRef.current, [entry.path]: { ...branch, open: !branch.open } });
  };
  type Row = TreeEntryV1 & { depth: number; moreFor?: string };
  const entries: Row[] = [];
  const append = (children: readonly TreeEntryV1[], depth: number) => {
    for (const entry of children) {
      if (!showHidden && entry.name.startsWith(".")) continue;
      entries.push({ ...entry, depth });
      const branch = branches[entry.path];
      if (entry.kind === "directory" && branch?.open) {
        append(branch.entries, depth + 1);
        if (branch.cursor || branch.loading || branch.failed) entries.push({ path: entry.path + "/", name: "", kind: "file", depth: depth + 1, moreFor: entry.path });
      }
    }
  };
  append(rootEntries, 0);
  const [scrollTop, setScrollTop] = useState(0);
  const listRef = useRef<HTMLDivElement>(null);
  const { t } = useI18n();
  const sentinelRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const root = listRef.current, sentinel = sentinelRef.current;
    if (!root || !sentinel || disabled || loading || failed || !hasMore) return;
    const observer = new IntersectionObserver(([entry]) => {
      if (entry.isIntersecting && root.clientHeight > 0) onLoadMore();
    }, { root, rootMargin: "0px 0px 160px 0px" });
    observer.observe(sentinel);
    return () => observer.disconnect();
  }, [disabled, loading, failed, hasMore, entries.length, onLoadMore]);
  const rowHeight = 32;
  const start = Math.min(Math.max(0, entries.length - 1), Math.max(0, Math.floor(scrollTop / rowHeight) - 8));
  const end = Math.min(entries.length, start + 120);
  return <div ref={listRef} className="m5-file-list m5-virtual-file-list" aria-busy={loading} onScroll={event => setScrollTop(event.currentTarget.scrollTop)} onKeyDown={event => {
    if (disabled || !["ArrowDown", "ArrowUp", "Home", "End"].includes(event.key)) return;
    const current = Number((event.target as HTMLElement).closest<HTMLButtonElement>("button[data-index]")?.dataset.index);
    if (!Number.isInteger(current)) return;
    event.preventDefault();
    const direction = event.key === "ArrowUp" || event.key === "End" ? -1 : 1;
    let next = event.key === "Home" ? 0 : event.key === "End" ? entries.length - 1 : current + direction;
    while (next >= 0 && next < entries.length && (entries[next].kind === "symlink" || entries[next].moreFor)) next += direction;
    if (next < 0 || next >= entries.length) return;
    const list = listRef.current;
    if (!list) return;
    if (next * rowHeight < list.scrollTop) list.scrollTop = next * rowHeight;
    else if ((next + 1) * rowHeight > list.scrollTop + list.clientHeight) list.scrollTop = (next + 1) * rowHeight - list.clientHeight;
    setScrollTop(list.scrollTop);
    requestAnimationFrame(() => list.querySelector<HTMLButtonElement>(`button[data-index="${next}"]`)?.focus());
  }}>
    <div aria-hidden="true" style={{ height: start * rowHeight }} />
    {entries.slice(start, end).map((entry, index) => entry.moreFor ? <TreeBranchMore key={entry.path} root={listRef} loading={branches[entry.moreFor].loading} failed={branches[entry.moreFor].failed} disabled={disabled} depth={entry.depth} onLoad={() => void loadBranch(entry.moreFor!)} /> : <button key={entry.path} data-index={start + index} type="button" className="m5-file-row" data-kind={entry.kind} aria-current={entry.kind === "file" && activePath === entry.path ? "true" : undefined} title={entry.path} aria-expanded={entry.kind === "directory" ? Boolean(branches[entry.path]?.open) : undefined} style={{ paddingLeft: 7 + entry.depth * 16 }} onClick={() => toggle(entry)} disabled={disabled || entry.kind === "symlink"}>
      <span className="m5-tree-chevron" aria-hidden="true">{entry.kind === "directory" ? (branches[entry.path]?.open ? "⌄" : "›") : ""}</span>
      <svg className="m5-file-glyph" aria-hidden="true" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">{entry.kind === "directory" ? <path d="M3 7V5a1 1 0 0 1 1-1h5l2 3h9a1 1 0 0 1 1 1v11a1 1 0 0 1-1 1H4a1 1 0 0 1-1-1V7Z" /> : <><path d="M14 3H5v18h14V8l-5-5Z M14 3v5h5" />{entry.kind === "symlink" ? <path d="m8 16 7-5m-4 0h4v4" /> : <path d="M8 13h8M8 17h6" />}</>}</svg>
      <span className="m5-tree-name">{entry.name}</span>
    </button>)}
    <div aria-hidden="true" style={{ height: (entries.length - end) * rowHeight }} />
    <div ref={sentinelRef} className="m5-tree-more">
      {loading ? <span role="status">{t("m5.navigation.loading")}</span> : failed ? <button type="button" disabled={disabled} onClick={onLoadMore}>{t("m5.navigation.retry")}</button> : null}
    </div>
  </div>;
});

function TreeBranchMore({ root, loading, failed, disabled, depth, onLoad }: { root: React.RefObject<HTMLDivElement | null>; loading: boolean; failed: boolean; disabled: boolean; depth: number; onLoad: () => void }) {
  const ref = useRef<HTMLDivElement>(null);
  const { t } = useI18n();
  useEffect(() => {
    if (!ref.current || !root.current || loading || failed || disabled) return;
    const observer = new IntersectionObserver(([entry]) => { if (entry.isIntersecting && root.current!.clientHeight > 0) onLoad(); }, {root: root.current, rootMargin: "160px"});
    observer.observe(ref.current);
    return () => observer.disconnect();
  }, [loading, failed, disabled, onLoad, root]);
  return <div ref={ref} className="m5-tree-branch-more" style={{paddingLeft: 7 + depth * 16}}>{failed ? <button disabled={disabled} onClick={onLoad}>{t("m5.navigation.retry")}</button> : loading ? <span role="status">{t("m5.navigation.loading")}</span> : null}</div>;
}

export function PanelTitle({ active, panel, onSelect, children }: { active: boolean; panel: WorkbenchPanel; onSelect: (panel: WorkbenchPanel) => void; children: ReactNode }) {
  return <button type="button" className="m5-panel-title" aria-pressed={active} onClick={() => onSelect(panel)}>{children}</button>;
}

export function EditorContent({ tab, readOnly, onChange, onSave }: { tab: EditorTab | null; readOnly: boolean; onChange: (text: string) => void; onSave: (path: string) => void }) {
  const { t } = useI18n();
  if (!tab) return <section className="m5-editor-welcome"><div className="m5-welcome-mark" aria-hidden="true">E</div><h2>Elsecrew</h2><p>{t("m5.editor.empty")}</p><span>{t("m5.editor.welcomeHint")}</span></section>;
  if (tab.mode === "binary") return <Empty text={t("m5.editor.binary")} />;
  if (tab.mode === "tooLarge") return <Empty text={t("m5.editor.large")} />;
  if (tab.saveState === "conflicted") {
    return <section className="m5-conflict"><strong>{t("m5.editor.conflict")}</strong><p>{t("m5.editor.conflictHelp")}</p><textarea aria-label={t("m5.editor.preserved")} readOnly value={tab.text} style={{ width: "100%", minHeight: 240, boxSizing: "border-box" }} /><details><summary>{t("m5.editor.details")}</summary><pre>{tab.technicalDetail ?? "external file change"}</pre></details></section>;
  }
  return <MonacoEditor tab={tab} readOnly={readOnly} onChange={onChange} onSave={onSave} />;
}

function MonacoEditor({ tab, readOnly, onChange, onSave }: { tab: EditorTab; readOnly: boolean; onChange: (text: string) => void; onSave: (path: string) => void }) {
  const { t } = useI18n();
  const [failed, setFailed] = useState(false);
  const [attempt, setAttempt] = useState(0);
  const host = useRef<HTMLDivElement>(null);
  const editor = useRef<Monaco.editor.IStandaloneCodeEditor | null>(null);
  const model = useRef<Monaco.editor.ITextModel | null>(null);
  const modelText = useRef<string | null>(null);
  const syncModel = useRef<(() => void) | null>(null);
  const labels = { save: t("m5.editor.commandSave"), name: t("m5.editor.accessibleName", { path: tab.path }) };
  const refreshLabels = useRef<(() => void) | null>(null);
  const latest = useRef({ tab, readOnly, onChange, onSave, labels });
  useLayoutEffect(() => { latest.current = { tab, readOnly, onChange, onSave, labels }; }, [tab, readOnly, onChange, onSave, labels.save, labels.name]);

  useEffect(() => {
    let cancelled = false;
    setFailed(false);
    const resources: { dispose(): void }[] = [];
    const cleanup = () => {
      refreshLabels.current = null;
      syncModel.current = null;
      for (const resource of resources.splice(0).reverse()) resource.dispose();
      editor.current = null;
      model.current = null;
      modelText.current = null;
    };
    void loadMonaco().then((monaco) => {
      if (cancelled || !host.current) return;
      const current = latest.current;
      let appliedReadOnly = current.readOnly || current.tab.saveState === "saving";
      monaco.editor.defineTheme("elsecrew", {
        base: "vs-dark", inherit: true, rules: [],
        colors: { "editor.background": "#111315", "editor.foreground": "#eceeed",
          "editorLineNumber.foreground": "#697078", "editorLineNumber.activeForeground": "#c3c7cd",
          "editorGutter.background": "#111315", "editor.lineHighlightBackground": "#191b1e",
          "editor.selectionBackground": "#354352", "editor.inactiveSelectionBackground": "#29323b" },
      });
      const nextEditor = monaco.editor.create(host.current, {
        model: null,
        theme: "elsecrew",
        lineHeight: 22,
        padding: { top: 12, bottom: 12 },
        scrollBeyondLastLine: false,
        lineNumbersMinChars: 4,
        renderLineHighlight: "gutter",
        automaticLayout: true,
        minimap: { enabled: false },
        fontSize: 13,
        readOnly: appliedReadOnly,
        ariaLabel: current.labels.name,
      });
      resources.push(nextEditor);
      editor.current = nextEditor;
      let saveAction: { dispose(): void } | null = null;
      resources.push({ dispose: () => { saveAction?.dispose(); saveAction = null; } });
      const registerLabels = () => {
        saveAction?.dispose();
        saveAction = null;
        nextEditor.updateOptions({ ariaLabel: latest.current.labels.name });
        saveAction = nextEditor.addAction({
          id: "elsecrew.save-file",
          label: latest.current.labels.save,
          keybindings: [monaco.KeyMod.CtrlCmd | monaco.KeyCode.KeyS],
          contextMenuGroupId: "9_cutcopypaste",
          run: () => {
            const current = latest.current;
            requestEditorSave(current.tab, current.readOnly, current.onSave);
          },
        });
      };
      registerLabels();
      refreshLabels.current = () => {
        try { registerLabels(); }
        catch { cleanup(); setFailed(true); }
      };
      // Keep one editor surface and one file model. Switching tabs releases the
      // previous model/undo buffer instead of retaining a hidden editor per file.
      let modelPath: string | null = null;
      let subscription: Monaco.IDisposable | null = null;
      let synchronizing = false;
      const releaseModel = () => {
        subscription?.dispose();
        subscription = null;
        nextEditor.setModel(null);
        model.current?.dispose();
        model.current = null;
        modelText.current = null;
        modelPath = null;
      };
      resources.push({ dispose: releaseModel });
      const synchronize = () => {
        const current = latest.current;
        if (modelPath !== current.tab.path) {
          releaseModel();
          const nextModel = monaco.editor.createModel(current.tab.text, languageFor(current.tab.path));
          model.current = nextModel;
          modelText.current = current.tab.text;
          modelPath = current.tab.path;
          nextEditor.setModel(nextModel);
          const ownerPath = current.tab.path;
          subscription = nextModel.onDidChangeContent(() => {
            if (synchronizing || latest.current.tab.path !== ownerPath) return;
            const text = nextModel.getValue();
            modelText.current = text;
            latest.current.onChange(text);
          });
        } else if (model.current && modelText.current !== current.tab.text) {
          synchronizing = true;
          try {
            model.current.setValue(current.tab.text);
            modelText.current = current.tab.text;
          } finally { synchronizing = false; }
        }
        const nextReadOnly = current.readOnly || current.tab.saveState === "saving";
        if (nextReadOnly !== appliedReadOnly) {
          nextEditor.updateOptions({ readOnly: nextReadOnly });
          appliedReadOnly = nextReadOnly;
        }
      };
      syncModel.current = () => {
        try { synchronize(); }
        catch { cleanup(); setFailed(true); }
      };
      synchronize();
    }).catch(() => {
      if (cancelled) return;
      cleanup();
      setFailed(true);
    });
    return () => {
      cancelled = true;
      cleanup();
    };
  }, [attempt]);

  useEffect(() => { refreshLabels.current?.(); }, [labels.save, labels.name]);

  // Install the current file before paint; an old model must never send edits
  // through the new tab's callback during a switch.
  useLayoutEffect(() => { syncModel.current?.(); }, [tab.path, tab.text, tab.saveState, readOnly]);


  return <>{failed && <section className="m5-error" role="alert"><strong>{t("m5.feedback.failed")}</strong><p>{t("m5.feedback.help")}</p><textarea aria-label={t("m5.editor.preserved")} readOnly value={tab.text} style={{ width: "100%", minHeight: 240, boxSizing: "border-box" }} /><button type="button" onClick={() => setAttempt(value => value + 1)}>{t("m5.feedback.retry")}</button></section>}<div className="m5-monaco" ref={host} style={failed ? { display: "none" } : undefined} /></>;
}

// Draft/output updates leave the reviewed diff unchanged. Stable callbacks and
// ordinary shallow comparison skip its rows; context and permission changes still render.
/** A read-only VS Code style comparison: only what changed is marked, side by
 * side (or in one column when narrow or chosen). Falls back to the plain
 * listing if the editor cannot load. */
/** A new or removed file has nothing to compare against: its text is shown
 * alone, read-only, instead of next to an empty side. Changed lines are told
 * apart by color only; the +/- gutter marks rendered as empty boxes here. */
export function MonacoDiff({ path, before, after, sideBySide, wrap, fit = false, prose = false, onFailed }: { path: string; before: string; after: string; sideBySide: boolean; wrap: boolean;
  /** A document: proportional letters, which read better for Korean text. */
  prose?: boolean;
  /** Grow to the content (stacked in the all-changes view) instead of filling the panel. */
  fit?: boolean; onFailed: () => void }) {
  const { t } = useI18n();
  const host = useRef<HTMLDivElement | null>(null);
  const view = useRef<Monaco.editor.IStandaloneDiffEditor | Monaco.editor.IStandaloneCodeEditor | null>(null);
  const single = before === "" || after === "";
  useEffect(() => {
    let cancelled = false;
    let models: Monaco.editor.ITextModel[] = [];
    void loadMonaco().then(monaco => {
      if (cancelled || !host.current) return;
      monaco.editor.defineTheme("elsecrew", {
        base: "vs-dark", inherit: true, rules: [],
        colors: { "editor.background": "#111315", "editor.foreground": "#eceeed",
          "editorLineNumber.foreground": "#697078", "editorLineNumber.activeForeground": "#c3c7cd",
          "editorGutter.background": "#111315", "editor.lineHighlightBackground": "#191b1e",
          "editor.selectionBackground": "#354352", "editor.inactiveSelectionBackground": "#29323b" },
      });
      const common = {
        theme: "elsecrew", readOnly: true, automaticLayout: true,
        minimap: { enabled: false }, scrollBeyondLastLine: false, fontSize: 13, lineHeight: 20,
        ...(prose ? { fontFamily: "-apple-system, BlinkMacSystemFont, \"Apple SD Gothic Neo\", \"Segoe UI\", sans-serif" } : {}),
        wordWrap: wrap ? "on" as const : "off" as const, renderOverviewRuler: false, ariaLabel: t("m5.diff.comparison"),
        // Stacked in the all-changes view, the page scrolls, not each file.
        ...(fit ? { scrollbar: { vertical: "hidden" as const, handleMouseWheel: false, alwaysConsumeMouseWheel: false }, stickyScroll: { enabled: false } } : {}),
      };
      const language = languageFor(path);
      const fitHeight = (height: number) => {
        if (host.current) host.current.style.height = `${Math.min(Math.max(height, 40), 6000)}px`;
        view.current?.layout();
      };
      if (single) {
        models = [monaco.editor.createModel(after || before, language)];
        const editor = monaco.editor.create(host.current, { ...common, model: models[0]!, glyphMargin: false, folding: false });
        view.current = editor;
        // Every line of a new file is added, of a removed file removed.
        editor.createDecorationsCollection([{
          range: new monaco.Range(1, 1, models[0]!.getLineCount(), 1),
          options: { isWholeLine: true, className: after ? "m5-line-added" : "m5-line-removed", linesDecorationsClassName: after ? "m5-gutter-added" : "m5-gutter-removed" },
        }]);
        if (fit) {
          // Start from the line count (content height is not measured yet),
          // then follow Monaco's measured height (wrapped lines).
          fitHeight(models[0]!.getLineCount() * 20 + 8);
          editor.onDidContentSizeChange(event => fitHeight(event.contentHeight));
          requestAnimationFrame(() => fitHeight(editor.getContentHeight()));
        }
        return;
      }
      const editor = monaco.editor.createDiffEditor(host.current, {
        ...common, originalEditable: false,
        renderSideBySide: fit ? false : sideBySide, useInlineViewWhenSpaceIsLimited: true, renderSideBySideInlineBreakpoint: 720,
        renderIndicators: false, renderMarginRevertIcon: false, diffWordWrap: wrap ? "on" : "off",
        hideUnchangedRegions: { enabled: true, contextLineCount: 3, minimumLineCount: 6, revealLineCount: 20 },
      });
      models = [monaco.editor.createModel(before, language), monaco.editor.createModel(after, language)];
      editor.setModel({ original: models[0]!, modified: models[1]! });
      view.current = editor;
      if (fit) {
        const size = () => fitHeight(Math.max(editor.getOriginalEditor().getContentHeight(), editor.getModifiedEditor().getContentHeight()));
        // Before the diff is computed, size by the longer side's line count.
        fitHeight(Math.max(models[0]!.getLineCount(), models[1]!.getLineCount()) * 20 + 8);
        editor.onDidUpdateDiff(() => requestAnimationFrame(size));
        editor.getModifiedEditor().onDidContentSizeChange(size);
      }
    }).catch(() => { if (!cancelled) onFailed(); });
    return () => { cancelled = true; view.current?.dispose(); view.current = null; for (const model of models) model.dispose(); };
    // Rebuilt only when the compared texts change; options update in place.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [path, before, after]);
  useEffect(() => {
    const options = { wordWrap: wrap ? "on" as const : "off" as const };
    if (single) (view.current as Monaco.editor.IStandaloneCodeEditor | null)?.updateOptions(options);
    else (view.current as Monaco.editor.IStandaloneDiffEditor | null)?.updateOptions({ ...options, renderSideBySide: fit ? false : sideBySide, diffWordWrap: options.wordWrap });
  }, [sideBySide, wrap, single, fit]);
  return <div ref={host} className={`m5-monaco-diff${fit ? " m5-monaco-fit" : ""}`} />;
}

export const DiffDetails = memo(function DiffDetails({ descriptor, text, canOpen, onOpen, canDiscuss, onDiscuss, onReturn }: { onReturn: () => void; descriptor: NonNullable<ReturnType<ProjectEditorController["snapshot"]>["diffFile"]>; text: string; canOpen: boolean; onOpen: (path: string) => void; canDiscuss: boolean; onDiscuss: (path: string, side?: "before" | "after", line?: number) => void }) {
  const { t } = useI18n();
  const indexed = useMemo(() => diffPages(text), [text]);
  // Pages are read as the reader scrolls; rows already shown stay in place.
  const [loaded, setLoaded] = useState(1);
  const [wrapLines, setWrapLines] = useState(true);
  const [diffMode, setDiffMode] = useState<"inline" | "split">("split");
  const sides = useMemo(() => descriptor.kind === "binary" ? null : diffSides(text), [descriptor.kind, text]);
  const [monacoFailed, setMonacoFailed] = useState(false);
  // A document (.md) reads as a document first: drawn, in a proportional font.
  const prose = isProsePath(descriptor.path);
  const [proseView, setProseView] = useState<"preview" | "changes">("preview");
  useEffect(() => { setLoaded(1); setMonacoFailed(false); setProseView("preview"); }, [descriptor, text]);
  const pages = indexed.pageCount;
  const shown = Math.min(loaded, pages);
  const rawRows = useMemo(() => Array.from({ length: shown }, (_, index) => indexed.page(index)).flat(), [indexed, shown]);
  // Keep unfamiliar or malformed metadata visible; only move known file headers out of the code view.
  const visible = useMemo(() => rawRows.filter(row => !rawRows.some(item => item.kind !== "metadata") || row.kind !== "metadata" || !/^(diff --git |index |--- |\+\+\+ )/.test(row.text)), [rawRows]);
  const metadataLabel = (value: string) => {
    const match = /^@@ -(\d+)(?:,\d+)? \+(\d+)(?:,\d+)? @@(.*)$/.exec(value);
    return match ? t("m5.diff.section", { before: match[1]!, after: match[2]! }) + match[3] : value;
  };
  const splitRows = useMemo(() => diffMode === "split" ? splitDiffPage(visible) : [], [visible, diffMode]);
  const lineNumber = (side: "before" | "after", line: number | null) => line !== null && canDiscuss
    ? <button type="button" className="m5-diff-line-reference" title={t("m5.diff.discussLine", { side: t(`m5.diff.${side}`), line })} aria-label={t("m5.diff.discussLine", { side: t(`m5.diff.${side}`), line })} onClick={() => onDiscuss(descriptor.path, side, line)}>{line}</button>
    : line;
  return (
    <section className="m5-diff-details" aria-label={t("m5.diff.title")}>
      <header className="m5-diff-header">
        <div className="m5-diff-identity"><strong>{descriptor.path}</strong><span>{t(`m5.diff.${descriptor.kind}`)}{descriptor.oldPath ? t("m5.diff.old", { path: descriptor.oldPath }) : ""}</span></div>
        <div className="m5-diff-actions">
          {descriptor.kind !== "deleted" && descriptor.worktreeVersion !== null && <button type="button" className="m5-icon-action" title={t("m5.diff.openEditor")} aria-label={t("m5.diff.openEditor")} disabled={!canOpen} onClick={() => onOpen(descriptor.path)}><svg viewBox="0 0 24 24" aria-hidden="true"><path d="m15 5 4 4M4 20l4-1L20 7a2.8 2.8 0 0 0-4-4L4 15z"/></svg></button>}
          <details className="m5-diff-options"><summary title={t("m5.diff.options")} aria-label={t("m5.diff.options")}>⋯</summary><div className="m5-diff-options-panel">
            {descriptor.kind !== "binary" && Boolean(text) && <div className="m5-diff-toolbar">
              <div className="m5-diff-mode" role="group" aria-label={t("m5.diff.displayMode")}>
                <button type="button" aria-pressed={diffMode === "inline"} onClick={() => setDiffMode("inline")}>{t("m5.diff.inline")}</button>
                <button type="button" aria-pressed={diffMode === "split"} onClick={() => setDiffMode("split")}>{t("m5.diff.split")}</button>
              </div>
              <label className="m5-diff-wrap"><input type="checkbox" checked={wrapLines} onChange={event => setWrapLines(event.target.checked)} />{t("m5.diff.wrapLines")}</label>
              <details className="m5-diff-help"><summary>{t("m5.diff.readingHelp")}</summary><p>{t("m5.diff.help")}</p>{canDiscuss && <p>{t("m5.diff.lineHelp")}</p>}</details>
            </div>}
            {canDiscuss && <button type="button" onClick={() => onDiscuss(descriptor.path)}>{t("m5.diff.discuss")}</button>}
            <button type="button" onClick={onReturn}>{t("m5.diff.returnEditor")}</button>
          </div></details>
        </div>
      </header>
      {prose && sides && descriptor.kind !== "binary" && descriptor.kind !== "deleted" && Boolean(text) && <div className="m5-prose-toggle" role="group" aria-label={t("m5.diff.displayMode")}>
        <button type="button" aria-pressed={proseView === "preview"} onClick={() => setProseView("preview")}>{t("m5.diff.prosePreview")}</button>
        <button type="button" aria-pressed={proseView === "changes"} onClick={() => setProseView("changes")}>{t("m5.diff.proseChanges")}</button>
      </div>}
      {descriptor.kind === "binary" ? <Empty text={t("m5.diff.binaryHelp")} /> : !text ? <Empty text={t("m5.diff.empty")} /> : prose && sides && proseView === "preview" && descriptor.kind !== "deleted"
        ? <div className="m5-prose-preview m5-md" tabIndex={0} aria-label={t("m5.diff.prosePreview")}><MarkdownView source={sides.after} /></div>
        : sides && !monacoFailed
        ? <MonacoDiff path={descriptor.path} before={sides.before} after={sides.after} sideBySide={diffMode === "split"} wrap={wrapLines} prose={prose} onFailed={() => setMonacoFailed(true)} />
        : <>
        <div className={`m5-diff-lines${wrapLines ? " m5-diff-lines-wrapped" : ""}${diffMode === "split" ? " m5-diff-split" : ""}`} tabIndex={0} aria-label={t("m5.diff.comparison")}>
          {diffMode === "split" ? <table>
            <colgroup><col className="m5-diff-number-column" /><col /><col className="m5-diff-number-column" /><col /></colgroup>
            <thead><tr><th scope="colgroup" colSpan={2}>{t("m5.diff.before")}</th><th scope="colgroup" colSpan={2}>{t("m5.diff.after")}</th></tr></thead>
            <tbody>{splitRows.map((row, index) => row.metadata
              ? <tr key={index} className="m5-diff-metadata"><td colSpan={4}><code>{metadataLabel(row.metadata.text)}</code></td></tr>
              : <tr key={index}>
                <td className={row.before ? `m5-diff-${row.before.kind}` : "m5-diff-absent"}>{lineNumber("before", row.before?.before ?? null)}</td>
                <td className={`m5-diff-side-code m5-diff-${row.before?.kind ?? "absent"}`}><code>{row.before?.text}</code></td>
                <td className={row.after ? `m5-diff-${row.after.kind}` : "m5-diff-absent"}>{lineNumber("after", row.after?.after ?? null)}</td>
                <td className={`m5-diff-side-code m5-diff-${row.after?.kind ?? "absent"}`}><code>{row.after?.text}</code></td>
              </tr>)}</tbody>
          </table> : <table><thead><tr><th scope="col">{t("m5.diff.before")}</th><th scope="col">{t("m5.diff.after")}</th><th scope="col">{t("m5.diff.content")}</th></tr></thead><tbody>
            {visible.map((row, index) => <tr key={index} className={`m5-diff-${row.kind}`}><td>{lineNumber("before", row.before)}</td><td>{lineNumber("after", row.after)}</td><td><code>{row.kind === "metadata" ? metadataLabel(row.text) : row.text}</code></td></tr>)}
          </tbody></table>}
          {shown < pages && <LoadMore key={shown} label={t("m5.diff.loadingMore", { shown: shown * DIFF_PAGE_ROWS, rows: indexed.rowCount })} onReach={() => setLoaded(shown + 1)} />}
        </div>
        <details className="m5-diff-source"><summary>{t("m5.diff.source")}</summary><pre>{rawRows.map(row => row.text).join("\n")}</pre></details>
      </>}
    </section>
  );
});


/** Loads the next block when it scrolls into view; remounted per block so a
 * still-visible marker asks again. The button keeps keyboard readers moving. */
function LoadMore({ label, onReach }: { label: string; onReach: () => void }) {
  const ref = useRef<HTMLDivElement>(null);
  const reach = useRef(onReach);
  reach.current = onReach;
  useEffect(() => {
    const node = ref.current;
    if (!node || typeof IntersectionObserver === "undefined") return;
    const observer = new IntersectionObserver(entries => { if (entries.some(entry => entry.isIntersecting)) { observer.disconnect(); reach.current(); } }, { rootMargin: "240px 0px" });
    observer.observe(node);
    return () => observer.disconnect();
  }, []);
  return <div ref={ref} className="m5-load-more"><button type="button" className="m5-secondary" onClick={() => reach.current()}>{label}</button></div>;
}

export function Empty({ text }: { text: string }) {
  return <p className="m5-empty">{text}</p>;
}
