import { Bridge, type WasmBindings } from "./bridge";
import { applyEvents, emptyHud, formatHud, mountHud } from "./hud";

declare global {
  interface Window {
    wasmBindings?: WasmBindings;
  }
}

function start(bindings: WasmBindings): void {
  const bridge = new Bridge(new bindings.Bridge());
  const el = mountHud(document);
  let state = emptyHud();
  let shown = "";
  const frame = (): void => {
    state = applyEvents(state, bridge.drain());
    const text = formatHud(state);
    if (text !== shown) el.textContent = shown = text;
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
