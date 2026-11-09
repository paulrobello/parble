import { startAudio } from "./audio/boot";
import { Bridge, type WasmBindings } from "./bridge";
import type { EditorDiag } from "./editor/model";
import { type Orbit, mountEditor } from "./editor/overlay";
import { mountFiles } from "./files";
import { applyEvents, controlsHint, emptyHud, formatHud, mountControlsHint, mountHud } from "./hud";
import { Library } from "./library";
import { mountFlyPointerLock } from "./pointer-lock";

declare global {
  interface Window {
    wasmBindings?: WasmBindings;
  }
}

type CameraState = { mode?: string; flyOnly?: boolean; orbit?: Orbit };
type ParbleState = { camera?: CameraState; editor?: EditorDiag };
const parbleState = (): ParbleState | undefined => (window as unknown as { __parble?: ParbleState }).__parble;
const cameraState = (): CameraState | undefined => parbleState()?.camera;

/** The editor overlay shows in the editor scenario; `editor-ui=0` keeps the bare P3-T4 editor (engine keys, files panel). */
export function editorUiWanted(search: string): boolean {
  const q = new URLSearchParams(search);
  return q.get("scenario") === "editor" && q.get("editor-ui") !== "0";
}

function start(bindings: WasmBindings): void {
  const bridge = new Bridge(new bindings.Bridge());
  const el = mountHud(document);
  const hint = mountControlsHint(document);
  mountFlyPointerLock(document, () => cameraState()?.mode ?? null);
  const library = "indexedDB" in window ? Library.open({ indexedDB: window.indexedDB }) : null;
  library?.catch((err) => console.warn("track library unavailable", err));
  const editor =
    editorUiWanted(location.search) && bindings.parbleEdit
      ? mountEditor({
          doc: document,
          win: window,
          send: (a) => bindings.parbleEdit?.(JSON.stringify(a)),
          post: (cmd) => bridge.post(cmd),
          library,
          orbit: () => cameraState()?.orbit ?? null,
        })
      : null;
  const files = editor ? null : mountFiles(document, (cmd) => bridge.post(cmd), library);
  const audio = startAudio(window);
  let state = emptyHud();
  let shown = "";
  const frame = (): void => {
    const events = bridge.drain();
    state = applyEvents(state, events);
    files?.handle(events);
    editor?.update(parbleState()?.editor ?? null, events);
    for (const e of events) if (e.type === "audioFrame") audio.consume(e);
    const text = formatHud(state);
    if (text !== shown) el.textContent = shown = text;
    const hintText = controlsHint(cameraState()?.flyOnly === true);
    if (hint.textContent !== hintText) hint.textContent = hintText;
    requestAnimationFrame(frame);
  };
  requestAnimationFrame(frame);
}

// Trunk sets window.wasmBindings and fires TrunkApplicationStarted once the wasm has initialised.
if (window.wasmBindings) start(window.wasmBindings);
else
  addEventListener(
    "TrunkApplicationStarted",
    () => {
      if (window.wasmBindings) start(window.wasmBindings);
    },
    { once: true },
  );
