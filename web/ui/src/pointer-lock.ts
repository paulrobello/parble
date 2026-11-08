// Pointer lock for the fly camera. Browsers grant requestPointerLock only inside a user-gesture
// handler, and the wasm app applies its cursor grab a frame later, so the page requests the lock
// from the same F key press the app reads to enter and leave fly.

interface LockDoc {
  pointerLockElement: Element | null;
  exitPointerLock(): void;
  getElementById(id: string): (Element & { requestPointerLock(): unknown }) | null;
  addEventListener(type: string, fn: (e: Event) => void, capture?: boolean): void;
}

/** The app's camera mode from `window.__parble`, or null before the app reports one. */
export type ModeOf = () => string | null;

export function mountFlyPointerLock(doc: LockDoc, mode: ModeOf, canvasId = "parble"): void {
  let releasing = false;
  doc.addEventListener(
    "keydown",
    (e) => {
      const k = e as KeyboardEvent;
      if (k.code !== "KeyF" || k.repeat || !k.isTrusted) return;
      const t = k.target as { tagName?: string } | null;
      if (t?.tagName === "INPUT" || t?.tagName === "TEXTAREA") return;
      const canvas = doc.getElementById(canvasId);
      if (!canvas) return;
      if (doc.pointerLockElement) {
        releasing = true;
        doc.exitPointerLock();
      } else if (mode() !== "fly") {
        Promise.resolve(canvas.requestPointerLock()).catch(() => {});
      }
    },
    true,
  );
  // The browser releases the lock on Esc and may swallow that key, so leave fly for the app too.
  doc.addEventListener("pointerlockchange", () => {
    if (doc.pointerLockElement) return;
    if (releasing) {
      releasing = false;
      return;
    }
    const canvas = doc.getElementById(canvasId);
    if (canvas && mode() === "fly") {
      for (const type of ["keydown", "keyup"])
        canvas.dispatchEvent(new KeyboardEvent(type, { code: "Escape", key: "Escape", bubbles: true }));
    }
  });
}
