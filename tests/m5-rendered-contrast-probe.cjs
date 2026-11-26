// Rendered contrast probe (PR #10 review I4): measures what the page actually
// draws, not token pairs. Runs inside the renderer smoke at each capture point,
// in the theme the smoke was started with (`M5_SMOKE_THEME`).
//
// - Text: every visible element with its own text, enabled, outside the code
//   editor and the preview stage: its color (with the opacity and brightness
//   filter of it and its ancestors) against the composited faces behind it.
//   4.5:1, or 3:1 for large text (24px, or 18.66px bold).
// - Inputs: the edge (or, without one, the face) against what is around it, 3:1.
// - Disabled buttons never wear the action color.
// - Segmented controls: the chosen one has an edge, and the focus ring is
//   drawn inside (not clipped by the track) — checked where `focus` is asked.
'use strict';

function parseColor(value) {
  if (!value || value === 'transparent') return { r: 0, g: 0, b: 0, a: 0 };
  let m = /^rgba?\(([^)]+)\)$/.exec(value);
  if (m) {
    const parts = m[1].split(/[\s,/]+/).filter(Boolean).map(Number);
    return { r: parts[0] / 255, g: parts[1] / 255, b: parts[2] / 255, a: parts.length > 3 ? parts[3] : 1 };
  }
  m = /^color\(srgb ([^)]+)\)$/.exec(value);
  if (m) {
    const [rgb, alpha] = m[1].split('/').map(s => s.trim());
    const [r, g, b] = rgb.split(/\s+/).map(Number);
    return { r, g, b, a: alpha === undefined ? 1 : Number(alpha) };
  }
  return null;
}
const over = (top, bottom) => ({
  r: top.r * top.a + bottom.r * (1 - top.a),
  g: top.g * top.a + bottom.g * (1 - top.a),
  b: top.b * top.a + bottom.b * (1 - top.a),
  a: 1,
});
const lum = c => { const f = v => v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4; return 0.2126 * f(c.r) + 0.7152 * f(c.g) + 0.0722 * f(c.b); };
const ratio = (a, b) => { const [x, y] = [lum(a), lum(b)].sort((p, q) => q - p); return (x + 0.05) / (y + 0.05); };
const hex = c => '#' + [c.r, c.g, c.b].map(v => Math.round(Math.min(1, Math.max(0, v)) * 255).toString(16).padStart(2, '0')).join('');
const brightness = filter => { const m = /brightness\(([\d.]+)\)/.exec(filter || ''); return m ? Number(m[1]) : 1; };
const scale = (c, k) => ({ r: Math.min(1, c.r * k), g: Math.min(1, c.g * k), b: Math.min(1, c.b * k), a: c.a });

function ancestors(element) {
  const chain = [];
  for (let node = element; node && node.nodeType === 1; node = node.parentElement) chain.push(node);
  return chain.reverse(); // root first
}

/** The face behind an element: its ancestors' backgrounds composited from the root. null when an image is involved. */
function faceBehind(element, includeSelf) {
  const page = parseColor(getComputedStyle(document.documentElement).backgroundColor);
  let face = page && page.a > 0 ? { ...page, a: 1 } : { r: 1, g: 1, b: 1, a: 1 };
  let light = 1;
  for (const node of ancestors(element)) {
    if (node === element && !includeSelf) break;
    const style = getComputedStyle(node);
    if (style.backgroundImage !== 'none' && node !== document.documentElement && node !== document.body) return null;
    const bg = parseColor(style.backgroundColor);
    if (bg === null) return null;
    if (bg.a > 0) face = over({ ...bg, a: bg.a * Number(style.opacity) }, face);
    light *= brightness(style.filter);
  }
  return scale(face, light);
}

function label(element) {
  const parts = [];
  for (let node = element; node && node.nodeType === 1 && parts.length < 4; node = node.parentElement) {
    let part = node.tagName.toLowerCase();
    if (node.id) part += '#' + node.id;
    const cls = [...node.classList].filter(c => c.startsWith('m5-')).slice(0, 2);
    if (cls.length) part += '.' + cls.join('.');
    parts.unshift(part);
  }
  return parts.join(' > ');
}

function visible(element) {
  if (!element.checkVisibility({ opacityProperty: true, visibilityProperty: true })) return false;
  const rect = element.getBoundingClientRect();
  return rect.width > 1 && rect.height > 1 && rect.bottom > 0 && rect.right > 0 && rect.top < innerHeight && rect.left < innerWidth;
}

function exempt(element) {
  // Disabled controls are outside the contrast requirement; code and the team's page are not app text.
  // Decorations hidden from assistive technology (a "/" between crumbs) are not text to read.
  return Boolean(element.closest('.monaco-editor, [data-testid=preview-stage], svg, [inert], [aria-disabled="true"], [aria-hidden="true"], :disabled, option'))
    || element.matches(':disabled');
}

function ownText(element) {
  let text = '';
  for (const child of element.childNodes) if (child.nodeType === 3) text += child.textContent;
  return text.trim();
}

function opacityChain(element) {
  let opacity = 1;
  for (const node of ancestors(element)) opacity *= Number(getComputedStyle(node).opacity);
  return opacity;
}

/** Lets finite animations and transitions (a toast coming in, a hover fading) reach their end state. */
async function settle(limitMs = 1500) {
  const finite = document.getAnimations().filter(animation => animation.playState === 'running' && Number.isFinite(animation.effect?.getComputedTiming?.().endTime ?? Infinity));
  await Promise.race([Promise.all(finite.map(animation => animation.finished.catch(() => undefined))), new Promise(done => setTimeout(done, limitMs))]);
}

/**
 * The ring `:focus-visible` would draw on a button, read from the page's own rules
 * (a hidden test window never matches :focus-visible itself).
 */
function focusRing(button) {
  let ring = null;
  const visit = rules => {
    for (const rule of rules) {
      if (rule.cssRules && !(rule instanceof CSSStyleRule)) { visit(rule.cssRules); continue; }
      if (!(rule instanceof CSSStyleRule) || !rule.selectorText.includes(':focus-visible')) continue;
      for (const selector of rule.selectorText.split(/,(?![^(]*\))/)) {
        if (!selector.includes(':focus-visible')) continue;
        let matches = false;
        try { matches = button.matches(selector.replaceAll(':focus-visible', '')); } catch { matches = false; }
        if (!matches) continue;
        const style = rule.style;
        ring = {
          style: style.outlineStyle || (style.outline ? style.outline.split(/\s+/).find(part => /^(solid|dashed|dotted|double|auto)$/.test(part)) : null) || ring?.style || null,
          width: parseFloat(style.outlineWidth || (style.outline ? style.outline : '')) || ring?.width || 0,
          offset: style.outlineOffset ? parseFloat(style.outlineOffset) : ring?.offset ?? 0,
        };
      }
    }
  };
  for (const sheet of document.styleSheets) { try { visit(sheet.cssRules); } catch { /* not readable */ } }
  return ring;
}

async function measureRenderedContrast(screen, { focus = false } = {}) {
  await settle();
  const failures = [];
  const actionColor = parseColor(getComputedStyle(document.documentElement).getPropertyValue('--action-bg').trim().replace(/^#(..)(..)(..)$/, (_m, r, g, b) => `rgb(${parseInt(r, 16)}, ${parseInt(g, 16)}, ${parseInt(b, 16)})`));
  let checked = 0;
  for (const element of document.querySelectorAll('body *')) {
    if (!(element instanceof HTMLElement) || !visible(element)) continue;
    const style = getComputedStyle(element);
    // Text
    const text = ownText(element);
    if (text && !exempt(element)) {
      const face = faceBehind(element, true);
      const color = parseColor(style.color);
      if (face && color) {
        const opacity = opacityChain(element);
        let light = 1;
        for (const node of ancestors(element)) light *= brightness(getComputedStyle(node).filter);
        const ink = over({ ...scale(color, light), a: color.a * opacity }, face);
        const size = parseFloat(style.fontSize);
        const large = size >= 24 || (size >= 18.66 && Number(style.fontWeight) >= 700);
        const min = large ? 3 : 4.5;
        const value = ratio(ink, face);
        checked++;
        if (value < min) failures.push({ screen, kind: 'text', at: label(element), text: text.slice(0, 40), ratio: Math.floor(value * 100) / 100, min, fg: hex(ink), bg: hex(face) });
      }
    }
    // Input edges
    if (element.matches('input:not([type=checkbox]):not([type=radio]):not([type=range]):not([type=hidden]), textarea, select') && !exempt(element)) {
      const outside = faceBehind(element, false);
      const own = faceBehind(element, true);
      const edge = parseColor(style.borderTopColor);
      if (outside && own && edge) {
        const width = parseFloat(style.borderTopWidth);
        const edgeValue = width > 0 && edge.a > 0 ? Math.max(ratio(over(edge, outside), outside), ratio(over(edge, own), own)) : 0;
        const faceValue = ratio(own, outside);
        checked++;
        if (Math.max(edgeValue, faceValue) < 3) failures.push({ screen, kind: 'input-edge', at: label(element), ratio: Math.floor(Math.max(edgeValue, faceValue) * 100) / 100, min: 3, fg: hex(edge), bg: hex(outside) });
      }
    }
    // Disabled buttons never wear the action color.
    if (element.matches('button:disabled') && actionColor) {
      const bg = parseColor(style.backgroundColor);
      if (bg && bg.a > 0.5 && Math.abs(bg.r - actionColor.r) < 0.02 && Math.abs(bg.g - actionColor.g) < 0.02 && Math.abs(bg.b - actionColor.b) < 0.02) {
        failures.push({ screen, kind: 'disabled-action', at: label(element), text: (element.textContent || '').trim().slice(0, 40) });
      }
    }
    // The chosen one of a segmented control carries an edge.
    if (element.matches(':is(.m5-segmented, .m5-seg, .m5-preview-viewport) button[aria-pressed="true"]') && !/inset/.test(style.boxShadow)) {
      failures.push({ screen, kind: 'segmented-chosen-edge', at: label(element) });
    }
  }
  if (focus) {
    for (const button of document.querySelectorAll(':is(.m5-segmented, .m5-seg, .m5-preview-viewport) button')) {
      if (!visible(button) || button.disabled) continue;
      const ring = focusRing(button);
      checked++;
      if (!ring || !ring.style || ring.style === 'none' || ring.width <= 0) { failures.push({ screen, kind: 'focus-ring-missing', at: label(button) }); continue; }
      if (ring.offset + ring.width > 0) {
        // A ring drawn outside the button must not be clipped by a wrapper.
        for (let node = button.parentElement; node && node !== document.body; node = node.parentElement) {
          if (getComputedStyle(node).overflow !== 'visible') { failures.push({ screen, kind: 'focus-ring-clipped', at: label(button) }); break; }
        }
      }
    }
  }
  return { screen, checked, failures };
}

module.exports = { measureRenderedContrast, focusRing, parseColor, ratio };
