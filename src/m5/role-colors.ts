/**
 * Role colours and icons (ADE 1차-나 L4b, custom roles R5). Every place that
 * shows a role — stage bar, member cards, task list, history — draws the
 * same colour for the same role ID: the built-in templates have fixed
 * colours, a role the person named gets one from a hash of its ID. Colours
 * are the CSS tokens `--role-1` … `--role-8` (styles.css).
 */
export const ROLE_COLOR_COUNT = 8;
const TEMPLATE_COLORS: ReadonlyMap<string, number> = new Map([
  ["coordinator", 1], ["planning", 2], ["coding", 3], ["review", 4], ["design", 5],
]);

/** FNV-1a over UTF-16 code units: stable across sessions and platforms. */
function hash(text: string): number {
  let value = 0x811c9dc5;
  for (let index = 0; index < text.length; index++) {
    value ^= text.charCodeAt(index);
    value = Math.imul(value, 0x01000193) >>> 0;
  }
  return value;
}

/** 1 … 8; the same role ID always gives the same colour. */
export function roleColorIndex(roleId: string): number {
  return TEMPLATE_COLORS.get(roleId) ?? (hash(roleId) % ROLE_COLOR_COUNT) + 1;
}

/** The icon letter: the first character of the shown name (never markup). */
export function roleInitial(name: string): string {
  const first = Array.from(name.trim())[0];
  return first ? first.toUpperCase() : "?";
}
