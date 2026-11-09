import { startAudio } from "./audio/boot";
import { Bridge, type WasmBindings } from "./bridge";
import { mountFiles } from "./files";
import { applyEvents, controlsHint, emptyHud, formatHud, mountControlsHint, mountHud } from "./hud";
import { Library } from "./library";
import { mountFlyPointerLock } from "./pointer-lock";

declare global {
  interface Window {
    wasmBindings?: WasmBindings;
  }
}

type CameraState = { mode?: string; flyOnly?: boolean };
const cameraState = (): CameraState | undefined =>
  (window as unknown as { __parble?: { camera?: CameraState } }).__parble?.camera;

function start(bindings: WasmBindings): void {
  const bridge = new Bridge(new bindings.Bridge());
  const el = mountHud(document);
  const hint = mountControlsHint(document);
  mountFlyPointerLock(document, () => cameraState()?.mode ?? null);
  const library = "indexedDB" in window ? Library.open({ indexedDB: window.indexedDB }) : null;
  library?.catch((err) => console.warn("track library unavailable", err));
  const files = mountFiles(document, (cmd) => bridge.post(cmd), library);
  const audio = startAudio(window);
  let state = emptyHud();
  let shown = "";
  const frame = (): void => {
    const events = bridge.drain();
    state = applyEvents(state, events);
    files.handle(events);
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
