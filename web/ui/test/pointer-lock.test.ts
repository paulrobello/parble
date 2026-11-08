import { describe, expect, it, vi } from "vitest";
import { mountFlyPointerLock } from "../src/pointer-lock";

function fakeDoc() {
  const handlers: Record<string, (e: Event) => void> = {};
  const dispatched: string[] = [];
  const canvas = {
    tagName: "CANVAS",
    requestPointerLock: vi.fn(() => Promise.resolve()),
    dispatchEvent: (e: Event) => {
      dispatched.push(`${e.type}:${(e as KeyboardEvent).code}`);
      return true;
    },
  };
  const doc = {
    pointerLockElement: null as Element | null,
    exitPointerLock: vi.fn(),
    getElementById: () => canvas as unknown as Element & { requestPointerLock(): unknown },
    addEventListener: (type: string, fn: (e: Event) => void) => {
      handlers[type] = fn;
    },
  };
  const key = (code: string, extra: Partial<KeyboardEvent> = {}) =>
    handlers.keydown?.({ code, repeat: false, isTrusted: true, target: canvas, ...extra } as unknown as Event);
  return { doc, canvas, handlers, dispatched, key };
}

class FakeKeyboardEvent {
  constructor(
    public type: string,
    init: { code: string },
  ) {
    this.code = init.code;
  }
  code: string;
}

describe("fly pointer lock", () => {
  it("requests the lock on F when not flying and releases it on the next F", () => {
    const f = fakeDoc();
    let mode = "director";
    mountFlyPointerLock(f.doc, () => mode);
    f.key("KeyF");
    expect(f.canvas.requestPointerLock).toHaveBeenCalledTimes(1);
    f.doc.pointerLockElement = f.canvas as unknown as Element;
    mode = "fly";
    f.key("KeyF");
    expect(f.doc.exitPointerLock).toHaveBeenCalledTimes(1);
    // The F release must not also send Escape.
    f.doc.pointerLockElement = null;
    f.handlers.pointerlockchange?.(new Event("pointerlockchange"));
    expect(f.dispatched).toEqual([]);
  });

  it("ignores other keys, repeats, synthetic events and text fields", () => {
    const f = fakeDoc();
    mountFlyPointerLock(f.doc, () => "director");
    f.key("KeyG");
    f.key("KeyF", { repeat: true });
    f.key("KeyF", { isTrusted: false });
    f.key("KeyF", { target: { tagName: "INPUT" } as unknown as EventTarget });
    expect(f.canvas.requestPointerLock).not.toHaveBeenCalled();
  });

  it("sends Escape to the app when the browser drops the lock while flying", () => {
    vi.stubGlobal("KeyboardEvent", FakeKeyboardEvent);
    const f = fakeDoc();
    mountFlyPointerLock(f.doc, () => "fly");
    f.handlers.pointerlockchange?.(new Event("pointerlockchange"));
    expect(f.dispatched).toEqual(["keydown:Escape", "keyup:Escape"]);
    vi.unstubAllGlobals();
  });
});
