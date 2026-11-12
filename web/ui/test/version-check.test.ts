// @vitest-environment happy-dom
// The new-version check: the poll interval key, the version.json body, VersionWatch's silent
// retries, focus throttle and once-per-id report against a fake fetch, and the update note.
import { describe, expect, it } from "vitest";
import { UpdateNote } from "../src/update-note";
import { MIN_GAP_MS, POLL_MS, VersionWatch, liveId, pollMs, updateWords } from "../src/version-check";

function harness(page: string | null = "aaaaaaaaaa") {
  let now = 1_000_000;
  let online = true;
  const urls: string[] = [];
  const found: string[] = [];
  let reply: () => Promise<Response> = async () => Response.json({ id: page });
  const watch = new VersionWatch({
    page,
    fetch: async (url, init) => {
      expect(init.cache).toBe("no-store");
      urls.push(url);
      return reply();
    },
    online: () => online,
    now: () => now,
    onNewer: (id) => found.push(id),
  });
  return {
    watch,
    urls,
    found,
    tick: (ms: number) => {
      now += ms;
    },
    setOnline: (v: boolean) => {
      online = v;
    },
    setReply: (r: () => Promise<Response>) => {
      reply = r;
    },
  };
}

describe("version check", () => {
  it("polls every 3 minutes by default, version-poll seconds from the address, at least 1 s", () => {
    expect(POLL_MS).toBe(180_000);
    expect(pollMs("")).toBe(POLL_MS);
    expect(pollMs("?version-poll=2")).toBe(2000);
    expect(pollMs("?scenario=showcase&version-poll=0.2")).toBe(1000);
    for (const bad of ["?version-poll=", "?version-poll=0", "?version-poll=-5", "?version-poll=soon"])
      expect(pollMs(bad), bad).toBe(POLL_MS);
  });

  it("counts only a hex id in version.json", () => {
    expect(liveId({ id: "9180bb81e5" })).toBe("9180bb81e5");
    for (const bad of [null, {}, { id: 7 }, { id: "<script>" }, { id: "" }, "9180bb81e5"])
      expect(liveId(bad)).toBeNull();
  });

  it("reports a newer id once, never the page's own, and every request busts the cache", async () => {
    const t = harness();
    await t.watch.check(true);
    expect(t.found).toEqual([]);
    t.setReply(async () => Response.json({ id: "bbbbbbbbbb" }));
    t.tick(1000);
    await t.watch.check(true);
    t.tick(1000);
    await t.watch.check(true);
    expect(t.found).toEqual(["bbbbbbbbbb"]);
    t.setReply(async () => Response.json({ id: "cccccccccc" }));
    t.tick(1000);
    await t.watch.check(true);
    expect(t.found).toEqual(["bbbbbbbbbb", "cccccccccc"]);
    expect(t.urls.every((u) => /^version\.json\?t=\d+$/.test(u))).toBe(true);
    expect(new Set(t.urls).size).toBe(t.urls.length);
  });

  it("is silent on errors, bad statuses, bad bodies and offline, and retries next time", async () => {
    const t = harness();
    t.setReply(async () => {
      throw new TypeError("Failed to fetch");
    });
    expect(await t.watch.check(true)).toBe("error");
    t.setReply(async () => new Response("down", { status: 503 }));
    t.tick(1000);
    expect(await t.watch.check(true)).toBe("bad-status");
    t.setReply(async () => Response.json({ id: "<script>" }));
    t.tick(1000);
    expect(await t.watch.check(true)).toBe("bad-body");
    t.setOnline(false);
    t.setReply(async () => Response.json({ id: "bbbbbbbbbb" }));
    t.tick(1000);
    expect(await t.watch.check(true)).toBe("offline");
    expect(t.urls.length).toBe(3);
    expect(t.found).toEqual([]);
    t.setOnline(true);
    t.tick(1000);
    expect(await t.watch.check(true)).toBe("newer");
    expect(t.found).toEqual(["bbbbbbbbbb"]);
  });

  it("throttles focus checks but not timed ones, skips a check in flight, and never polls without a build id", async () => {
    const t = harness();
    await t.watch.check();
    t.tick(MIN_GAP_MS - 1);
    expect(await t.watch.check()).toBe("throttled");
    expect(await t.watch.check(true)).toBe("same");
    let release: () => void = () => {};
    t.setReply(() => new Promise((r) => (release = () => r(Response.json({ id: "aaaaaaaaaa" })))));
    const first = t.watch.check(true);
    expect(await t.watch.check(true)).toBe("busy");
    release();
    expect(await first).toBe("same");
    const none = harness(null);
    expect(await none.watch.check(true)).toBe("no-build");
    expect(none.urls).toEqual([]);
  });
});

describe("update note", () => {
  it("words warn while the editor has unsaved edits", () => {
    expect(updateWords(false)).toEqual({ sub: "Reload to play it.", action: "Reload" });
    expect(updateWords(true).action).toBe("Reload anyway");
    expect(updateWords(true).sub).toMatch(/unsaved edits/);
  });

  it("shows once with Reload and Later, follows the unsaved state, and reloads on click", () => {
    let unsaved = false;
    let reloads = 0;
    const note = new UpdateNote(
      document,
      () => unsaved,
      () => reloads++,
    );
    note.sync();
    expect(document.querySelector("#parble-update")).toBeNull();
    note.show();
    note.show();
    expect(document.querySelectorAll("#parble-update").length).toBe(1);
    const el = document.querySelector("#parble-update") as HTMLElement;
    expect(el.getAttribute("role")).toBe("status");
    expect(el.textContent).toContain("New version available");
    const go = el.querySelector("button.go") as HTMLButtonElement;
    expect(go.textContent).toBe("Reload");
    unsaved = true;
    note.sync();
    expect(go.textContent).toBe("Reload anyway");
    expect(el.classList.contains("caution")).toBe(true);
    go.click();
    expect(reloads).toBe(1);
    (el.querySelector("button.x") as HTMLButtonElement).click();
    expect(document.querySelector("#parble-update")).toBeNull();
  });
});
