// Line icons for the strip (the decision sketch's set, .impeccable/mocks/decision/model-pick.html),
// one <symbol> sprite referenced by <use href="#pe-…">.

const SYMBOLS: Record<string, string> = {
  sel: '<path d="M5 3l10 6-4.5 1.2L8.5 15z"/>',
  plc: '<path d="M10 2.5l7 4v7l-7 4-7-4v-7z M10 7v6 M7 10h6"/>',
  del: '<path d="M4 6h12 M8 6V4h4v2 M5.5 6l.8 10h7.4l.8-10"/>',
  rot: '<path d="M15.5 10a5.5 5.5 0 1 1-1.8-4.1 M14 2.5v3.5h-3.5"/>',
  mir: '<path d="M10 3v14 M7 6L3 10l4 4z M13 6l4 4-4 4z"/>',
  und: '<path d="M7 5L3.5 8.5 7 12 M3.5 8.5H12a4 4 0 0 1 0 8H8"/>',
  red: '<path d="M13 5l3.5 3.5L13 12 M16.5 8.5H8a4 4 0 0 0 0 8h4"/>',
  lib: '<path d="M4 3v14 M8 3v14 M12 4l4 12"/>',
  r1: '<circle cx="10" cy="5" r="2.5"/><path d="M10 9v8 M7 14l3 3 3-3"/>',
  ra: '<circle cx="4.5" cy="4.5" r="1.8"/><circle cx="10" cy="4.5" r="1.8"/><circle cx="15.5" cy="4.5" r="1.8"/><path d="M4.5 8.5v8 M10 8.5v8 M15.5 8.5v8"/>',
  int: '<circle cx="10" cy="11" r="6"/><path d="M10 11V7.5 M8 2.5h4"/>',
  pau: '<path d="M7 4v12 M13 4v12"/>',
  rst: '<path d="M4 10a6 6 0 1 0 2-4.5 M4 3v3.5h3.5"/>',
  cam: '<path d="M3 6h3l1.5-2h5L14 6h3v10H3z"/><circle cx="10" cy="11" r="2.5"/>',
  col: '<path d="M7 3h6v4H7z M8 7v8 M12 7v8 M4 17h12"/>',
  chl: '<path d="M12 5l-5 5 5 5"/>',
  chr: '<path d="M8 5l5 5-5 5"/>',
  chd: '<path d="M5 8l5 5 5-5"/>',
  snd: '<path d="M3 8v4h3l4 3V5L6 8z M13 7.5a3.5 3.5 0 0 1 0 5 M15.5 5a7 7 0 0 1 0 10"/>',
  play: '<path d="M6 4l10 6-10 6z" fill="currentColor"/>',
  stop: '<path d="M5 5h10v10H5z" fill="currentColor"/>',
  side: '<path d="M3 4h14v12H3z M12 4v12"/>',
  imp: '<path d="M10 3v10 M6 9l4 4 4-4 M4 16h12"/>',
  exp: '<path d="M10 13V3 M6 7l4-4 4 4 M4 16h12"/>',
  save: '<path d="M4 4h10l2 2v10H4z M7 4v4h6V4 M7 16v-5h6v5"/>',
  x: '<path d="M5 5l10 10 M15 5L5 15"/>',
};

export function iconSprite(doc: Document): SVGSVGElement {
  const svg = doc.createElementNS("http://www.w3.org/2000/svg", "svg");
  svg.setAttribute("width", "0");
  svg.setAttribute("height", "0");
  svg.setAttribute("aria-hidden", "true");
  svg.style.position = "absolute";
  svg.innerHTML = `<defs>${Object.entries(SYMBOLS)
    .map(([id, d]) => `<symbol id="pe-${id}" viewBox="0 0 20 20">${d}</symbol>`)
    .join("")}</defs>`;
  return svg;
}

/** `<svg class="i"><use href="#pe-id"/></svg>` markup. */
export const icon = (id: string): string => `<svg class="i" aria-hidden="true"><use href="#pe-${id}"/></svg>`;
