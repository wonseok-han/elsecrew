import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { useI18n } from "../i18n.tsx";
import type { DiffEntryV1, DiffFileDescriptorV1 } from "./contracts/diff-v1.ts";
import { diffSides } from "./diff-view.ts";

/**
 * The changes list and the all-changes view, modelled on Orca's source
 * control panel (user direction 2026-11-18): one row per file with its name,
 * a dim folder, `+added −removed` and a colored status letter; a list or tree
 * view; and "view all", every file's comparison one under another.
 */

const KIND_LETTER: Record<string, string> = { added: "A", modified: "M", deleted: "D", renamed: "R", binary: "B" };
const VIEW_KEY = "elsecrew.changes.view.v1";
const PAGE = 300;

type ViewMode = "list" | "tree";

function readViewMode(): ViewMode {
  try { return localStorage.getItem(VIEW_KEY) === "tree" ? "tree" : "list"; } catch { return "list"; }
}

function fileName(path: string): string {
  const parts = path.split("/");
  return parts[parts.length - 1] ?? path;
}

function folderOf(path: string): string {
  return path.includes("/") ? path.slice(0, path.lastIndexOf("/")) : "";
}

export function ChangeKindLetter({ entry }: { entry: DiffEntryV1 }) {
  const { t } = useI18n();
  const label = t(`m5.diff.${entry.kind}`);
  return <span className={`m5-change-kind m5-kind-${entry.kind}`} title={label} aria-label={label}>{KIND_LETTER[entry.kind] ?? "?"}</span>;
}

export function LineCounts({ entry }: { entry: Pick<DiffEntryV1, "added" | "removed"> }) {
  const { t } = useI18n();
  if (entry.added === undefined || entry.removed === undefined) return null;
  return <span className="m5-line-counts" aria-label={t("m5.changes.lineCounts", { added: entry.added, removed: entry.removed })}>
    {entry.added > 0 && <span className="m5-lines-added">+{entry.added}</span>}
    {entry.removed > 0 && <span className="m5-lines-removed">−{entry.removed}</span>}
  </span>;
}

/** Folders in name order after top-level files; files in name order. */
function changeGroups(entries: readonly DiffEntryV1[]): [string, DiffEntryV1[]][] {
  const groups = new Map<string, DiffEntryV1[]>();
  for (const entry of entries) {
    const folder = folderOf(entry.path);
    groups.set(folder, [...(groups.get(folder) ?? []), entry]);
  }
  return [...groups.entries()]
    .sort(([left], [right]) => (left === "" ? -1 : right === "" ? 1 : left.localeCompare(right)))
    .map(([folder, files]) => [folder, [...files].sort((a, b) => fileName(a.path).localeCompare(fileName(b.path)))]);
}

export function ChangeList({ entries, selected, disabled, onOpen, onViewAll, allOpen, onRefresh, refreshing }: {
  entries: readonly DiffEntryV1[]; selected?: string; disabled: boolean;
  onOpen: (path: string) => void;
  onRefresh: () => void; refreshing: boolean;
  /** Opens (or closes) every file's comparison in the editor area. */
  onViewAll: () => void; allOpen: boolean;
}) {
  const { t } = useI18n();
  const [query, setQuery] = useState("");
  const [mode, setMode] = useState<ViewMode>(readViewMode);
  const [shown, setShown] = useState(PAGE);
  const [collapsed, setCollapsed] = useState<ReadonlySet<string>>(() => new Set());
  const needle = query.trim().toLowerCase();
  const matches = useMemo(() => entries.filter(entry => !needle || entry.path.toLowerCase().includes(needle) || entry.oldPath?.toLowerCase().includes(needle)), [entries, needle]);
  const visible = matches.slice(0, shown);
  const totals = useMemo(() => entries.reduce((sum, entry) => ({ added: sum.added + (entry.added ?? 0), removed: sum.removed + (entry.removed ?? 0) }), { added: 0, removed: 0 }), [entries]);
  useEffect(() => setShown(PAGE), [entries, needle]);
  const choose = (next: ViewMode) => { setMode(next); try { localStorage.setItem(VIEW_KEY, next); } catch { /* per-viewer convenience only */ } };
  const row = (entry: DiffEntryV1, nested: boolean) => <button key={`${entry.kind}:${entry.path}`} type="button"
    className={`m5-change-row${nested ? " m5-change-nested" : ""}`} aria-current={selected === entry.path ? "true" : undefined}
    title={entry.oldPath ? `${entry.oldPath} → ${entry.path}` : entry.path} onClick={() => onOpen(entry.path)} disabled={disabled}>
    <span className={`m5-change-file${entry.kind === "deleted" ? " m5-change-file-deleted" : ""}`}>
      <strong>{fileName(entry.path)}</strong>
      {mode === "list" && folderOf(entry.path) && <small>{folderOf(entry.path)}</small>}
      {entry.oldPath && <small>{t("m5.diff.old", { path: entry.oldPath })}</small>}
    </span>
    <LineCounts entry={entry} />
    <ChangeKindLetter entry={entry} />
  </button>;
  const refresh = <button type="button" className="m5-icon-button" title={t("m5.editor.refresh")} aria-label={t("m5.editor.refresh")} disabled={disabled || refreshing} onClick={onRefresh}>↻</button>;
  if (entries.length === 0) return <>
    <div className="m5-changes-toolbar"><span /><div className="m5-changes-actions">{refresh}</div></div>
    <p className="m5-empty">{t("m5.editor.noChanges")}</p>
  </>;
  return <>
    <div className="m5-changes-toolbar">
      <span className="m5-changes-summary"><LineCounts entry={totals} /></span>
      <div className="m5-changes-actions">
        {refresh}
        <button type="button" className={allOpen ? "m5-active" : undefined} aria-pressed={allOpen} disabled={disabled} onClick={onViewAll}>{t("m5.changes.viewAll")}</button>
        <div className="m5-segmented" role="group" aria-label={t("m5.changes.viewMode")}>
          <button type="button" aria-pressed={mode === "list"} onClick={() => choose("list")}>{t("m5.changes.list")}</button>
          <button type="button" aria-pressed={mode === "tree"} onClick={() => choose("tree")}>{t("m5.changes.tree")}</button>
        </div>
      </div>
    </div>
    <section className="m5-file-search" aria-label={t("m5.diff.filter")}>
      <input type="search" aria-label={t("m5.diff.filter")} placeholder={t("m5.diff.filter")} value={query} maxLength={256} disabled={disabled}
        onChange={event => setQuery(event.target.value)} onKeyDown={event => { if (event.key === "Escape") { event.preventDefault(); setQuery(""); } }} />
      {needle && <p role="status">{t("m5.diff.fileCount", { shown: matches.length, total: entries.length })}</p>}
    </section>
    <div className="m5-change-list">
      {mode === "list"
        ? [...visible].sort((a, b) => a.path.localeCompare(b.path)).map(entry => row(entry, false))
        : changeGroups(visible).map(([folder, files]) => <div key={folder || "."} className="m5-change-group">
          {folder && <button type="button" className="m5-change-folder" aria-expanded={!collapsed.has(folder)} onClick={() => setCollapsed(old => { const next = new Set(old); if (next.has(folder)) next.delete(folder); else next.add(folder); return next; })}>
            <span aria-hidden="true">{collapsed.has(folder) ? "▸" : "▾"}</span><span className="m5-change-folder-name">{folder}</span><small>{files.length}</small>
          </button>}
          {!collapsed.has(folder) && files.map(entry => row(entry, Boolean(folder)))}
        </div>)}
      {matches.length === 0 && <p className="m5-empty">{t("m5.diff.noMatches")}</p>}
      {visible.length < matches.length && <button type="button" className="m5-secondary" onClick={() => setShown(shown + PAGE)}>{t("m5.diff.fileRange", { last: visible.length, total: matches.length })}</button>}
    </div>
  </>;
}

type Loaded = { state: "loading" } | { state: "failed" } | { state: "ready"; descriptor: DiffFileDescriptorV1; text: string };

/** Every changed file's comparison, one under another; each loads as it
 * scrolls into view, one at a time, and can be folded away. */
export function AllChanges({ entries, focusPath, read, renderComparison, onOpenFile, onClose }: {
  entries: readonly DiffEntryV1[];
  /** The file picked in the list: scrolled to and unfolded. */
  focusPath: { path: string; at: number } | null;
  read: (path: string) => Promise<{ descriptor: DiffFileDescriptorV1; text: string } | null>;
  renderComparison: (path: string, before: string, after: string) => ReactNode;
  onOpenFile: (path: string) => void;
  onClose: () => void;
}) {
  const { t } = useI18n();
  const sorted = useMemo(() => [...entries].sort((a, b) => a.path.localeCompare(b.path)), [entries]);
  const [loaded, setLoaded] = useState<Record<string, Loaded>>({});
  const [folded, setFolded] = useState<ReadonlySet<string>>(() => new Set());
  const [wanted, setWanted] = useState<ReadonlySet<string>>(() => new Set());
  const sections = useRef(new Map<string, HTMLElement>());
  const busy = useRef(false);
  // The list refreshes often while agents work: only files whose change
  // looks different (kind or line counts) are read again.
  const signatures = useRef(new Map<string, string>());
  useEffect(() => {
    const next = new Map(sorted.map(entry => [entry.path, `${entry.kind}:${entry.oldPath ?? ""}:${entry.added ?? "?"}:${entry.removed ?? "?"}`]));
    const stale = [...signatures.current.entries()].filter(([path, signature]) => next.get(path) !== signature).map(([path]) => path);
    signatures.current = next;
    if (stale.length) setLoaded(old => { const kept = { ...old }; for (const path of stale) delete kept[path]; return kept; });
  }, [sorted]);
  useEffect(() => {
    const observer = new IntersectionObserver(items => {
      const seen = items.filter(item => item.isIntersecting).map(item => (item.target as HTMLElement).dataset.path!).filter(Boolean);
      if (seen.length) setWanted(old => new Set([...old, ...seen]));
    }, { rootMargin: "400px 0px" });
    for (const element of sections.current.values()) observer.observe(element);
    return () => observer.disconnect();
  }, [sorted]);
  // One read at a time, in list order, for sections that came into view.
  useEffect(() => {
    if (busy.current) return;
    const next = sorted.find(entry => wanted.has(entry.path) && !loaded[entry.path] && entry.kind !== "binary" && !folded.has(entry.path));
    if (!next) return;
    busy.current = true;
    setLoaded(old => ({ ...old, [next.path]: { state: "loading" } }));
    void read(next.path).then(result => {
      setLoaded(old => ({ ...old, [next.path]: result ? { state: "ready", ...result } : { state: "failed" } }));
    }).catch(() => setLoaded(old => ({ ...old, [next.path]: { state: "failed" } })))
      .finally(() => { busy.current = false; setWanted(old => new Set(old)); });
  }, [sorted, wanted, loaded, folded, read]);
  useEffect(() => {
    if (!focusPath) return;
    setFolded(old => { const next = new Set(old); next.delete(focusPath.path); return next; });
    sections.current.get(focusPath.path)?.scrollIntoView({ block: "start" });
  }, [focusPath]);
  const totals = sorted.reduce((sum, entry) => ({ added: sum.added + (entry.added ?? 0), removed: sum.removed + (entry.removed ?? 0) }), { added: 0, removed: 0 });
  const allFolded = sorted.length > 0 && sorted.every(entry => folded.has(entry.path));
  const body = (entry: DiffEntryV1): ReactNode => {
    if (entry.kind === "binary") return <p className="m5-empty">{t("m5.diff.binaryHelp")}</p>;
    const value = loaded[entry.path];
    if (!value || value.state === "loading") return <p className="m5-empty" role="status">{t("m5.changes.loading")}</p>;
    if (value.state === "failed") return <p className="m5-empty" role="alert">{t("m5.changes.loadFailed")}</p>;
    if (!value.text) return <p className="m5-empty">{t("m5.diff.empty")}</p>;
    const sides = diffSides(value.text);
    return sides ? renderComparison(entry.path, sides.before, sides.after) : <pre className="m5-all-changes-raw">{value.text}</pre>;
  };
  return <section className="m5-all-changes" aria-label={t("m5.changes.allTitle")}>
    <header className="m5-all-changes-head">
      <span className="m5-changes-summary">{t("m5.changes.summary", { count: sorted.length })} <LineCounts entry={totals} /></span>
      <span className="m5-all-changes-actions">
        <button type="button" className="m5-link-button" onClick={() => setFolded(new Set(sorted.map(entry => entry.path)))} disabled={allFolded}>{t("m5.changes.foldAll")}</button>
        <button type="button" className="m5-link-button" onClick={() => setFolded(new Set())} disabled={folded.size === 0}>{t("m5.changes.unfoldAll")}</button>
        <button type="button" className="m5-link-button" aria-label={t("m5.changes.close")} title={t("m5.changes.close")} onClick={onClose}>✕</button>
      </span>
    </header>
    <div className="m5-all-changes-scroll">
      {sorted.map(entry => <article key={entry.path} className="m5-all-changes-file" data-path={entry.path}
        ref={element => { if (element) sections.current.set(entry.path, element); else sections.current.delete(entry.path); }}>
        <header>
          <button type="button" className="m5-all-changes-toggle" aria-expanded={!folded.has(entry.path)} title={entry.oldPath ? `${entry.oldPath} → ${entry.path}` : entry.path}
            onClick={() => setFolded(old => { const next = new Set(old); if (next.has(entry.path)) next.delete(entry.path); else next.add(entry.path); return next; })}>
            <span className="m5-all-changes-chevron" aria-hidden="true">{folded.has(entry.path) ? "›" : "⌄"}</span>
            <span className={`m5-all-changes-path${entry.kind === "deleted" ? " m5-change-file-deleted" : ""}`}>
              {folderOf(entry.path) && <span className="m5-all-changes-folder">{folderOf(entry.path)}/</span>}<strong>{fileName(entry.path)}</strong>
              {entry.oldPath && <small>{t("m5.diff.old", { path: entry.oldPath })}</small>}
            </span>
            <LineCounts entry={entry} />
          </button>
          {entry.kind !== "deleted" && <button type="button" className="m5-link-button m5-all-changes-open" onClick={() => onOpenFile(entry.path)}>{t("m5.changes.open")}</button>}
        </header>
        {!folded.has(entry.path) && <div className="m5-all-changes-body">{body(entry)}</div>}
      </article>)}
    </div>
  </section>;
}
