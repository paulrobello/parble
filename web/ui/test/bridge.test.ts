import { describe, expect, it } from "vitest";
import {
  Bridge,
  type BridgeCommand,
  type BridgeEvent,
  COMMAND_TYPES,
  decodeCommand,
  decodeEvent,
  EVENT_TYPES,
  PROTOCOL,
} from "../src/bridge";
import fixtures from "../src/bridge-fixtures.json";

describe("bridge fixtures (shared with crates/parble-app/src/bridge.rs)", () => {
  it("decode every command, one per type in declaration order", () => {
    const decoded = fixtures.commands.map((v) => decodeCommand(v));
    expect(decoded).toEqual(fixtures.commands);
    expect(decoded.map((c) => c?.type)).toEqual([...COMMAND_TYPES]);
  });

  it("decode every event, one per type in declaration order", () => {
    const decoded = fixtures.events.map((v) => decodeEvent(v));
    expect(decoded).toEqual(fixtures.events);
    expect(decoded.map((e) => e?.type)).toEqual([...EVENT_TYPES]);
  });

  it("refuse every command Rust refuses", () => {
    for (const v of fixtures.rejectedCommands) expect(decodeCommand(v), JSON.stringify(v)).toBeNull();
  });

  it("agree on the protocol version", () => {
    expect(fixtures.protocol).toBe(PROTOCOL);
  });
});

describe("Bridge", () => {
  it("posts JSON commands and decodes drained events, skipping unknown ones", () => {
    const posted: string[] = [];
    const events: BridgeEvent[] = [{ type: "frame", tick: 3, fps: 60 }];
    const bridge = new Bridge({
      postCommand: (json) => {
        posted.push(json);
      },
      drainEvents: () => JSON.stringify([...events, { type: "mystery" }]),
    });
    const ping: BridgeCommand = { type: "ping", id: 9 };
    bridge.post(ping);
    expect(posted.map((j) => JSON.parse(j))).toEqual([ping]);
    expect(bridge.drain()).toEqual(events);
  });
});
