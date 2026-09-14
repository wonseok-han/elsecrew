export const PANEL_IDS = ["files", "editor", "changes", "conversation"] as const;
export type DockPanel = typeof PANEL_IDS[number];
export interface DockLayout { version: 1; order: DockPanel[]; widths: Record<DockPanel, number> }
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
    return { version: 1, order: [...value.order], widths: Object.fromEntries(PANEL_IDS.map(id => [id, value.widths[id]])) as Record<DockPanel, number> };
  } catch { return defaultDockLayout(); }
}
export function moveDockPanel(layout: DockLayout, source: DockPanel, target: DockPanel): DockLayout {
  if (source === target || !layout.order.includes(source) || !layout.order.includes(target)) return layout;
  const order = layout.order.filter(id => id !== source);
  order.splice(layout.order.indexOf(target), 0, source);
  return { ...layout, order };
}
export function resizeDockPanels(layout: DockLayout, left: DockPanel, right: DockPanel, delta: number): DockLayout {
  if (!Number.isFinite(delta) || left === right || !PANEL_IDS.includes(left) || !PANEL_IDS.includes(right)) return layout;
  const pair = layout.widths[left] + layout.widths[right];
  const width = Math.max(.08, Math.min(pair - .08, layout.widths[left] + delta));
  return { ...layout, widths: { ...layout.widths, [left]: width, [right]: Math.max(.08, pair - width) } };
}
