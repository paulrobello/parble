// The new-version check (ported from hex-world web/ui/src/version-check-model.ts). The build inlines
// its id into index.html as window.__PARBLE_BUILD and writes the same id to version.json
// (scripts/web/version-dist.mjs); this polls that file and reports an id that differs from the
// page's. update-note.ts runs it in the page.

/** How often the live build is checked: every 3 minutes, plus on focus and when the tab shows. */
export const POLL_MS = 180_000;
/** A focus or visibility check this soon after the last one is skipped. */
export const MIN_GAP_MS = 15_000;

/** The poll interval: `version-poll=<seconds>` in the address (scripts and tests), else POLL_MS. */
export function pollMs(search: string): number {
  const raw = new URLSearchParams(search).get("version-poll");
  const s = raw == null || raw === "" ? Number.NaN : Number(raw);
  return Number.isFinite(s) && s > 0 ? Math.max(1000, Math.round(s * 1000)) : POLL_MS;
}

/** The id a version.json body names, or null when the body is not one. */
export function liveId(body: unknown): string | null {
  const id = (body as { id?: unknown } | null)?.id;
  return typeof id === "string" && /^[0-9a-f]{6,64}$/.test(id) ? id : null;
}

export interface WatchDeps {
  /** The page's own build id (window.__PARBLE_BUILD); with none the check never runs. */
  page: string | null;
  fetch: (url: string, init: RequestInit) => Promise<Response>;
  online: () => boolean;
  now: () => number;
  /** A live build other than the page's: called once per new id. */
  onNewer: (id: string) => void;
}

/** What one check did: skipped (no build id, a check already running, throttled or offline), or the
 *  fetch's result (the page's own id, a newer id reported now or already reported, a non-2xx status,
 *  a body without a hex id, or a failed request). */
export type CheckOutcome =
  | "no-build"
  | "busy"
  | "throttled"
  | "offline"
  | "same"
  | "newer"
  | "reported"
  | "bad-status"
  | "bad-body"
  | "error";

export class VersionWatch {
  private readonly deps: WatchDeps;
  private busy = false;
  private last = Number.NEGATIVE_INFINITY;
  private found: string | null = null;

  constructor(deps: WatchDeps) {
    this.deps = deps;
  }

  /**
   * One check of version.json, fetched past every cache. A timed check (`timed`) always runs; a focus
   * or visibility check is skipped within MIN_GAP_MS of the last. Offline, a failed request, a bad
   * status or body are silent: the next check retries.
   */
  async check(timed = false): Promise<CheckOutcome> {
    const d = this.deps;
    const now = d.now();
    if (!d.page) return "no-build";
    if (this.busy) return "busy";
    if (!timed && now - this.last < MIN_GAP_MS) return "throttled";
    if (!d.online()) return "offline";
    this.busy = true;
    this.last = now;
    try {
      const res = await d.fetch(`version.json?t=${now}`, { cache: "no-store" });
      if (!res.ok) return "bad-status";
      const id = liveId(await res.json());
      if (!id) return "bad-body";
      if (id === d.page) return "same";
      if (id === this.found) return "reported";
      this.found = id;
      d.onNewer(id);
      return "newer";
    } catch {
      return "error";
    } finally {
      this.busy = false;
    }
  }
}

/** The update note's words: plain, or a warning while the editor holds unsaved edits. */
export function updateWords(unsaved: boolean): { sub: string; action: string } {
  return unsaved
    ? { sub: "This track has unsaved edits. Save it to the library first.", action: "Reload anyway" }
    : { sub: "Reload to play it.", action: "Reload" };
}
