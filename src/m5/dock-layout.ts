export const PANEL_IDS = ["files", "editor", "changes", "conversation"] as const;
export type DockPanel = typeof PANEL_IDS[number];
export type DockNode = { type: "tabs"; panels: DockPanel[]; active: DockPanel } | { type: "split"; axis: "horizontal" | "vertical"; ratio: number; first: DockNode; second: DockNode };
export interface DockLayout { version: 1; order: DockPanel[]; widths: Record<DockPanel, number>; hidden?: DockPanel[]; direction?: "horizontal" | "vertical"; tabbed?: boolean; activeTab?: DockPanel; tree?: DockNode }
export const DOCK_STORAGE_KEY = "hello-agent.m5.dock.v1";
export function defaultDockLayout(): DockLayout {
  return { version: 1, order: [...PANEL_IDS], widths: { files: .17, editor: .36, changes: .21, conversation: .26 } };
}
export function parseDockLayout(raw: string | null): DockLayout {
  try {
    const value = JSON.parse(raw ?? "null");
    if (value?.version !== 1 || !Array.isArray(value.order) || value.order.length !== 4 || new Set(value.order).size !== 4 || value.order.some((id: unknown) => !PANEL_IDS.includes(id as DockPanel))) return defaultDockLayout();
    if (!value.widths || PANEL_IDS.some(id => !Number.isFinite(value.widths[id]) || value.widths[id] < .08 || value.widths[id] > .76)) return defaultDockLayout();
    if (Math.abs(PANEL_IDS.reduce((sum, id) => sum + value.widths[id], 0) - 1) > .000001) return defaultDockLayout();
    if (value.hidden !== undefined && (!Array.isArray(value.hidden) || value.hidden.length >= PANEL_IDS.length || new Set(value.hidden).size !== value.hidden.length || value.hidden.some((id: unknown) => !PANEL_IDS.includes(id as DockPanel)))) return defaultDockLayout();
    if (value.direction !== undefined && value.direction !== "horizontal" && value.direction !== "vertical") return defaultDockLayout();
    if (value.tabbed !== undefined && typeof value.tabbed !== "boolean") return defaultDockLayout();
    if (value.activeTab !== undefined && !PANEL_IDS.includes(value.activeTab)) return defaultDockLayout();
    if (value.tree !== undefined) {
      const ids: DockPanel[] = [];
      const valid = (node: DockNode, depth = 0): boolean => {
        if (!node || depth > 6) return false;
        if (node.type === "tabs") {
          if (!Array.isArray(node.panels) || !node.panels.length || node.panels.length > 4 || !node.panels.includes(node.active) || node.panels.some(id => !PANEL_IDS.includes(id))) return false;
          ids.push(...node.panels); return true;
        }
        return node.type === "split" && ["horizontal", "vertical"].includes(node.axis) && Number.isFinite(node.ratio) && node.ratio >= .08 && node.ratio <= .92 && valid(node.first, depth + 1) && valid(node.second, depth + 1);
      };
      if (!valid(value.tree) || ids.length !== 4 || new Set(ids).size !== 4) return defaultDockLayout();
    }
    return { version: 1, order: [...value.order], widths: Object.fromEntries(PANEL_IDS.map(id => [id, value.widths[id]])) as Record<DockPanel, number>, ...(value.hidden ? { hidden: [...value.hidden] } : {}), ...(value.direction ? { direction: value.direction } : {}), ...(value.tabbed !== undefined ? { tabbed: value.tabbed } : {}), ...(value.activeTab ? { activeTab: value.activeTab } : {}), ...(value.tree ? { tree: value.tree } : {}) };
  } catch { return defaultDockLayout(); }
}

export function visibleDockPanels(layout: DockLayout, available: readonly DockPanel[]): DockPanel[] {
  const visible = layout.order.filter(id => available.includes(id) && !layout.hidden?.includes(id));
  // A chat can disappear while only its conversation was visible. Always retain a usable panel.
  return visible.length ? visible : available.includes("editor") ? ["editor"] : available.slice(0, 1);
}

export function activeDockTab(layout: DockLayout, visible: readonly DockPanel[]): DockPanel | undefined {
  return layout.activeTab && visible.includes(layout.activeTab) ? layout.activeTab
    : visible.includes("editor") ? "editor" : visible[0];
}

export function toggleDockPanel(layout: DockLayout, panel: DockPanel, available: readonly DockPanel[]): DockLayout {
  if (!available.includes(panel)) return layout;
  const visible = visibleDockPanels(layout, available);
  if (visible.includes(panel) && visible.length === 1) return layout;
  const hidden = new Set(layout.hidden ?? []);
  // Materialize the fallback visible panel before updating the saved choice.
  for (const id of visible) hidden.delete(id);
  if (visible.includes(panel)) hidden.add(panel); else hidden.delete(panel);
  return { ...layout, hidden: [...hidden] };
}

export function focusDockPanel(layout: DockLayout, panel: DockPanel): DockLayout {
  return PANEL_IDS.includes(panel) ? { ...layout, hidden: PANEL_IDS.filter(id => id !== panel) } : layout;
}
export function moveDockPanel(layout: DockLayout, source: DockPanel, target: DockPanel): DockLayout {
  if (source === target || !layout.order.includes(source) || !layout.order.includes(target)) return layout;
  const order = layout.order.filter(id => id !== source);
  order.splice(layout.order.indexOf(target), 0, source);
  return { ...layout, order };
}
export function revealDockPanel(layout: DockLayout, panel: DockPanel): DockLayout {
  if (!PANEL_IDS.includes(panel)) return layout;
  const visible = { ...layout, ...(layout.hidden ? { hidden: layout.hidden.filter(id => id !== panel) } : {}) };
  return layout.tree ? selectDockTab(visible, panel) : layout.tabbed ? { ...visible, activeTab: panel } : visible;
}
export function resizeDockPanels(layout: DockLayout, left: DockPanel, right: DockPanel, delta: number): DockLayout {
  if (!Number.isFinite(delta) || left === right || !PANEL_IDS.includes(left) || !PANEL_IDS.includes(right)) return layout;
  const pair = layout.widths[left] + layout.widths[right];
  const width = Math.max(.08, Math.min(pair - .08, layout.widths[left] + delta));
  return { ...layout, widths: { ...layout.widths, [left]: width, [right]: Math.max(.08, pair - width) } };
}

export type DockPlacement = "left" | "right" | "top" | "bottom" | "tab";
export function dockTree(layout: DockLayout): DockNode {
  if (layout.tree) return layout.tree;
  if (layout.tabbed) return { type: "tabs", panels: [...layout.order], active: layout.activeTab ?? "editor" };
  const split = (panels: DockPanel[]): DockNode => panels.length === 1
    ? { type: "tabs", panels, active: panels[0] }
    : { type: "split", axis: layout.direction ?? "horizontal", ratio: layout.widths[panels[0]] / panels.reduce((sum, id) => sum + layout.widths[id], 0), first: split(panels.slice(0, 1)), second: split(panels.slice(1)) };
  return split(layout.order);
}
function dockIds(node: DockNode): DockPanel[] {
  return node.type === "tabs" ? node.panels : [...dockIds(node.first), ...dockIds(node.second)];
}
export function placeDockPanel(layout: DockLayout, source: DockPanel, target: DockPanel, placement: DockPlacement): DockLayout {
  if (source === target || !PANEL_IDS.includes(source) || !PANEL_IDS.includes(target) || !["left", "right", "top", "bottom", "tab"].includes(placement)) return layout;
  const remove = (node: DockNode): DockNode | undefined => {
    if (node.type === "tabs") {
      const panels = node.panels.filter(id => id !== source);
      return panels.length ? { ...node, panels, active: panels.includes(node.active) ? node.active : panels[0] } : undefined;
    }
    const first = remove(node.first), second = remove(node.second);
    return first && second ? { ...node, first, second } : first ?? second;
  };
  const insert = (node: DockNode): DockNode => {
    if (node.type === "split") return { ...node, first: insert(node.first), second: insert(node.second) };
    if (!node.panels.includes(target)) return node;
    if (placement === "tab") return { ...node, panels: [...node.panels, source], active: source };
    const added: DockNode = { type: "tabs", panels: [source], active: source };
    const before = placement === "left" || placement === "top";
    const targetGroup = { ...node, active: target };
    return { type: "split", axis: placement === "left" || placement === "right" ? "horizontal" : "vertical", ratio: .5, first: before ? added : targetGroup, second: before ? targetGroup : added };
  };
  const tree = insert(remove(dockTree(layout))!);
  return { ...layout, tree, order: dockIds(tree), ...(layout.hidden ? { hidden: layout.hidden.filter(id => id !== source && id !== target) } : {}) };
}
export function selectDockTab(layout: DockLayout, panel: DockPanel): DockLayout {
  const select = (node: DockNode): DockNode => node.type === "split" ? { ...node, first: select(node.first), second: select(node.second) }
    : node.panels.includes(panel) ? { ...node, active: panel } : node;
  return { ...layout, tree: select(dockTree(layout)) };
}
export function resizeDockSplit(layout: DockLayout, path: string, ratio: number): DockLayout {
  if (!/^[01]*$/.test(path) || !Number.isFinite(ratio)) return layout;
  const resize = (node: DockNode, rest: string): DockNode => node.type !== "split" ? node : !rest ? { ...node, ratio: Math.max(.1, Math.min(.9, ratio)) }
    : rest[0] === "0" ? { ...node, first: resize(node.first, rest.slice(1)) } : { ...node, second: resize(node.second, rest.slice(1)) };
  return { ...layout, tree: resize(dockTree(layout), path) };
}
export interface DockRect { x: number; y: number; width: number; height: number }
export function dockGeometry(layout: DockLayout, available: readonly DockPanel[]) {
  const eligible = visibleDockPanels(layout, available);
  const groups: { panels: DockPanel[]; active: DockPanel; rect: DockRect }[] = [];
  const separators: { path: string; axis: "horizontal" | "vertical"; ratio: number; rect: DockRect; parent: DockRect }[] = [];
  const walk = (node: DockNode, rect: DockRect, path: string) => {
    if (node.type === "tabs") {
      const panels = node.panels.filter(id => eligible.includes(id));
      if (panels.length) groups.push({ panels, active: panels.includes(node.active) ? node.active : panels.includes("editor") ? "editor" : panels[0], rect });
      return;
    }
    const first = dockIds(node.first).some(id => eligible.includes(id)), second = dockIds(node.second).some(id => eligible.includes(id));
    if (!first || !second) { if (first || second) walk(first ? node.first : node.second, rect, path + (first ? "0" : "1")); return; }
    const horizontal = node.axis === "horizontal";
    const a = { ...rect, ...(horizontal ? { width: rect.width * node.ratio } : { height: rect.height * node.ratio }) };
    const b = { ...rect, ...(horizontal ? { x: rect.x + a.width, width: rect.width - a.width } : { y: rect.y + a.height, height: rect.height - a.height }) };
    separators.push({ path, axis: node.axis, ratio: node.ratio, parent: rect, rect: horizontal ? { x: b.x, y: rect.y, width: 0, height: rect.height } : { x: rect.x, y: b.y, width: rect.width, height: 0 } });
    walk(node.first, a, path + "0"); walk(node.second, b, path + "1");
  };
  walk(dockTree(layout), { x: 0, y: 0, width: 1, height: 1 }, "");
  return { groups, separators };
}
