// The browser track library (P3-T3): `.parble` files in IndexedDB. Each record keeps the file text
// exactly as Rust exported it, so loading from the library replays the same bytes; the record's own
// name and save time live beside it and never reach the sim.

import { type ParbleFile, parseParble } from "./parble";

export const DB_NAME = "parble";
export const STORE = "tracks";
const DB_VERSION = 1;

export interface TrackRecord {
  id: string;
  name: string;
  /** `.parble` text. */
  json: string;
  /** Epoch milliseconds of the last save or rename. */
  saved: number;
}

export type TrackSummary = Omit<TrackRecord, "json">;

/** Asked before a delete; resolve false to keep the track. */
export type Confirm = (name: string) => boolean | Promise<boolean>;

export interface LibraryDeps {
  indexedDB: IDBFactory;
  now?: () => number;
  newId?: () => string;
  dbName?: string;
}

const done = <T>(req: IDBRequest<T>): Promise<T> =>
  new Promise((resolve, reject) => {
    req.onsuccess = () => resolve(req.result);
    req.onerror = () => reject(req.error);
  });

const committed = (tx: IDBTransaction): Promise<void> =>
  new Promise((resolve, reject) => {
    tx.oncomplete = () => resolve();
    tx.onabort = () => reject(tx.error);
    tx.onerror = () => reject(tx.error);
  });

function checked(json: string): ParbleFile {
  const file = parseParble(json);
  if (!file) throw new Error("not a valid .parble file");
  return file;
}

/** `json` with `metadata.name` replaced, re-serialized in the canonical pretty form. */
export function withName(json: string, name: string): string {
  const file = checked(json);
  return `${JSON.stringify({ ...file, metadata: { ...file.metadata, name } }, null, 2)}\n`;
}

export class Library {
  readonly #db: IDBDatabase;
  readonly #now: () => number;
  readonly #newId: () => string;

  private constructor(db: IDBDatabase, now: () => number, newId: () => string) {
    this.#db = db;
    this.#now = now;
    this.#newId = newId;
  }

  static async open(deps: LibraryDeps): Promise<Library> {
    const req = deps.indexedDB.open(deps.dbName ?? DB_NAME, DB_VERSION);
    req.onupgradeneeded = () => {
      if (!req.result.objectStoreNames.contains(STORE)) req.result.createObjectStore(STORE, { keyPath: "id" });
    };
    const db = await done(req);
    return new Library(db, deps.now ?? Date.now, deps.newId ?? (() => crypto.randomUUID()));
  }

  close(): void {
    this.#db.close();
  }

  async #write(rec: TrackRecord): Promise<void> {
    const tx = this.#db.transaction(STORE, "readwrite");
    tx.objectStore(STORE).put(rec);
    await committed(tx);
  }

  /** Every track, newest save first. */
  async list(): Promise<TrackSummary[]> {
    const tx = this.#db.transaction(STORE, "readonly");
    const all = (await done(tx.objectStore(STORE).getAll())) as TrackRecord[];
    return all
      .map(({ id, name, saved }) => ({ id, name, saved }))
      .sort((a, b) => b.saved - a.saved || a.id.localeCompare(b.id));
  }

  async get(id: string): Promise<TrackRecord | null> {
    const tx = this.#db.transaction(STORE, "readonly");
    return ((await done(tx.objectStore(STORE).get(id))) as TrackRecord | undefined) ?? null;
  }

  /** Saves `json` as a new track, or over `id`. Refuses text Rust would refuse. */
  async save(json: string, id?: string): Promise<TrackRecord> {
    const file = checked(json);
    const rec: TrackRecord = { id: id ?? this.#newId(), name: file.metadata.name, json, saved: this.#now() };
    await this.#write(rec);
    return rec;
  }

  async rename(id: string, name: string): Promise<TrackRecord> {
    const rec = await this.get(id);
    if (!rec) throw new Error(`no track ${id}`);
    const next = { ...rec, name, json: withName(rec.json, name), saved: this.#now() };
    await this.#write(next);
    return next;
  }

  /** A copy under a new id, named "<name> copy". */
  async duplicate(id: string): Promise<TrackRecord> {
    const rec = await this.get(id);
    if (!rec) throw new Error(`no track ${id}`);
    const name = `${rec.name} copy`;
    return this.save(withName(rec.json, name));
  }

  /** Deletes `id` once `confirm` agrees. Returns whether it was deleted. */
  async remove(id: string, confirm: Confirm): Promise<boolean> {
    const rec = await this.get(id);
    if (!rec || !(await confirm(rec.name))) return false;
    const tx = this.#db.transaction(STORE, "readwrite");
    tx.objectStore(STORE).delete(id);
    await committed(tx);
    return true;
  }
}
