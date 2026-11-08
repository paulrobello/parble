import { IDBFactory } from "fake-indexeddb";
import { describe, expect, it } from "vitest";
import { Library } from "../src/library";
import { parseParble } from "../src/parble";
import fixtures from "../src/parble-fixtures.json";

const text = (i: number): string => `${JSON.stringify(fixtures.valid[i]?.file, null, 2)}\n`;

async function open(): Promise<Library> {
  let t = 1_000;
  let n = 0;
  return Library.open({ indexedDB: new IDBFactory(), now: () => (t += 10), newId: () => `t${++n}` });
}

describe("Library (IndexedDB)", () => {
  it("saves and lists newest first, keeping the exact text", async () => {
    const lib = await open();
    const a = await lib.save(text(0));
    const b = await lib.save(text(1));
    expect((await lib.list()).map((s) => s.id)).toEqual([b.id, a.id]);
    expect((await lib.list())[1]).toEqual({ id: a.id, name: "Round-trip demo", saved: a.saved });
    expect((await lib.get(a.id))?.json).toBe(text(0));
    const over = await lib.save(text(2), a.id);
    expect(over.id).toBe(a.id);
    expect(await lib.list()).toHaveLength(2);
    lib.close();
  });

  it("refuses text Rust would refuse", async () => {
    const lib = await open();
    await expect(lib.save("{}")).rejects.toThrow(/not a valid/);
    await expect(lib.save(JSON.stringify(fixtures.rejected[0]?.file))).rejects.toThrow();
    expect(await lib.list()).toEqual([]);
  });

  it("renames the record and the file metadata", async () => {
    const lib = await open();
    const a = await lib.save(text(0));
    const r = await lib.rename(a.id, "Renamed");
    expect(r.name).toBe("Renamed");
    const file = parseParble(r.json);
    expect(file?.metadata.name).toBe("Renamed");
    expect({ ...file, metadata: undefined }).toEqual({ ...fixtures.valid[0]?.file, metadata: undefined });
    await expect(lib.rename("missing", "x")).rejects.toThrow(/no track/);
  });

  it("duplicates under a new id", async () => {
    const lib = await open();
    const a = await lib.save(text(0));
    const d = await lib.duplicate(a.id);
    expect(d.id).not.toBe(a.id);
    expect(d.name).toBe("Round-trip demo copy");
    expect(parseParble(d.json)?.placements).toEqual(fixtures.valid[0]?.file.placements);
    expect(await lib.list()).toHaveLength(2);
  });

  it("deletes only after confirmation", async () => {
    const lib = await open();
    const a = await lib.save(text(0));
    const asked: string[] = [];
    expect(
      await lib.remove(a.id, (name) => {
        asked.push(name);
        return false;
      }),
    ).toBe(false);
    expect(await lib.get(a.id)).not.toBeNull();
    expect(await lib.remove(a.id, async () => true)).toBe(true);
    expect(await lib.get(a.id)).toBeNull();
    expect(asked).toEqual(["Round-trip demo"]);
    expect(await lib.remove(a.id, () => true)).toBe(false);
  });
});
