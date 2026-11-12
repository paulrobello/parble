// "New version available": polls version.json (version-check.ts) and, once a newer build is live,
// shows a Walnut Marquetry inlay (.impeccable/surfaces/web-ui-src-editor.md) at the top centre that
// stays until Reload or dismiss. Above the title screen (z-index 40) so a first-run visitor sees it.
import { fontFaces } from "./editor/style";
import { type CheckOutcome, VersionWatch, pollMs, updateWords } from "./version-check";

declare global {
  interface Window {
    /** This page's build id, inlined by scripts/web/version-dist.mjs. */
    __PARBLE_BUILD?: string;
  }
}

const CSS = `
#parble-update{position:fixed;left:50%;top:14px;z-index:50;display:flex;align-items:center;gap:12px;padding:10px 10px 10px 14px;
  transform:translate(-50%,-8px);opacity:0;transition:opacity .3s ease-out,transform .3s ease-out;
  background:rgba(34,22,15,.94);border:1px solid rgba(217,194,160,.55);border-radius:6px;color:#f0e6d6;
  box-shadow:0 10px 28px rgba(0,0,0,.45),0 2px 4px rgba(0,0,0,.4);
  font:400 13px/1.25 "Alegreya Sans",ui-sans-serif,system-ui,sans-serif;-webkit-font-smoothing:antialiased}
#parble-update.shown{opacity:1;transform:translate(-50%,0)}
#parble-update::before{content:"";position:absolute;inset:4px;border:1px solid rgba(217,194,160,.16);border-radius:3px;pointer-events:none}
#parble-update .pin{width:7px;height:7px;border-radius:50%;flex:none;
  background:radial-gradient(circle at 35% 30%,#e8c985,#c49a52 55%,#7d5f2c);box-shadow:0 1px 2px rgba(0,0,0,.6)}
#parble-update.caution .pin{background:radial-gradient(circle at 35% 30%,#f2b5a6,#e0705a 55%,#8a3b2c)}
#parble-update p{margin:0;display:flex;flex-direction:column;gap:2px}
#parble-update p span{font-weight:700}
#parble-update small{font-size:12px;color:#d9c2a0}
#parble-update button{font:inherit;cursor:pointer;border-radius:4px;padding:6px 12px;position:relative}
#parble-update .go{font-weight:700;color:#22160f;background:#c49a52;border:1px solid #e8c985}
#parble-update .go:hover{background:#d4ab63}
#parble-update .x{color:#d9c2a0;background:none;border:1px solid transparent;padding:6px 8px}
#parble-update .x:hover{border-color:rgba(217,194,160,.28)}
#parble-update :focus-visible{outline:2px solid #a8c8d8;outline-offset:2px}
@media (prefers-reduced-motion:reduce){#parble-update{transition:none}}
`;

export class UpdateNote {
  private readonly doc: Document;
  private readonly unsaved: () => boolean;
  private readonly reload: () => void;
  private el: HTMLElement | null = null;
  private sub: HTMLElement | null = null;
  private btn: HTMLButtonElement | null = null;

  constructor(doc: Document, unsaved: () => boolean, reload: () => void = () => location.reload()) {
    this.doc = doc;
    this.unsaved = unsaved;
    this.reload = reload;
  }

  /** Shows the note (once; later calls refresh its words). */
  show(): void {
    if (!this.el) {
      const d = this.doc;
      const style = d.createElement("style");
      style.id = "parble-update-style";
      style.textContent = `${fontFaces(d.defaultView ?? window)}\n${CSS}`;
      d.head.append(style);
      const el = d.createElement("div");
      el.id = "parble-update";
      el.setAttribute("role", "status");
      el.setAttribute("aria-live", "polite");
      const pin = d.createElement("i");
      pin.className = "pin";
      const p = d.createElement("p");
      const title = d.createElement("span");
      title.textContent = "New version available";
      this.sub = d.createElement("small");
      p.append(title, this.sub);
      this.btn = d.createElement("button");
      this.btn.type = "button";
      this.btn.className = "go";
      this.btn.addEventListener("click", () => this.reload());
      const x = d.createElement("button");
      x.type = "button";
      x.className = "x";
      x.textContent = "Later";
      x.addEventListener("click", () => this.hide());
      // Keys typed here stay here, not in the engine's camera and playback map.
      el.addEventListener("keydown", (e) => e.stopPropagation());
      el.append(pin, p, this.btn, x);
      d.body.append(el);
      this.el = el;
      void el.offsetWidth;
      el.classList.add("shown");
    }
    this.sync();
  }

  /** Re-reads the editor's unsaved state into the words. */
  sync(): void {
    if (!this.el || !this.sub || !this.btn) return;
    const unsaved = this.unsaved();
    const w = updateWords(unsaved);
    this.sub.textContent = w.sub;
    this.btn.textContent = w.action;
    this.el.classList.toggle("caution", unsaved);
  }

  /** "Later": hides the note until the next newer build. */
  hide(): void {
    this.el?.remove();
    this.el = this.sub = this.btn = null;
  }
}

/** Dispatched on window after every check, `detail` its CheckOutcome, for e2e/tests/version.spec.mjs. */
export const CHECK_EVENT = "parbleversioncheck";

/** Checks on an interval (`version-poll` seconds in the address, else 3 minutes), on focus, when the
 *  tab shows again and when the browser comes back online. */
export function watchVersion(note: UpdateNote, win: Window = window): VersionWatch {
  const watch = new VersionWatch({
    page: typeof win.__PARBLE_BUILD === "string" ? win.__PARBLE_BUILD : null,
    fetch: (url, init) => win.fetch(url, init),
    online: () => win.navigator.onLine !== false,
    now: Date.now,
    onNewer: () => note.show(),
  });
  const run = (timed = false): void =>
    void watch
      .check(timed)
      .then((outcome) => win.dispatchEvent(new CustomEvent<CheckOutcome>(CHECK_EVENT, { detail: outcome })));
  win.setInterval(() => run(true), pollMs(win.location.search));
  win.addEventListener("focus", () => run());
  win.addEventListener("online", () => run());
  win.document.addEventListener("visibilitychange", () => {
    if (win.document.visibilityState === "visible") run();
  });
  return watch;
}
