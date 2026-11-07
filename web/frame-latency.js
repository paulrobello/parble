// Frame-latency limit for the web build, on by default: at most 3 rendered frames' WebGPU work stay
// unfinished before the next animation frame runs, the bound wgpu's native surface keeps with its
// maximum frame latency. `?frame_latency=N` (1 to 8) changes the limit; `?frame_latency=off` (or 0)
// patches nothing.
// Why: when animation frames come faster than the browser's GPU side can consume them (uncapped
// Chrome), Chrome queues about 180 ms of WebGPU work and then blocks the page's main thread for
// 25-90 ms while the compositor waits for that work to be scheduled (docs/perf/stall-tail/). At
// 60 Hz the queue stays at one or two frames and the limit changes nothing measurable.
(function () {
  var DEFAULT_LIMIT = 3;
  var key = new URLSearchParams(location.search).get("frame_latency");
  var source = key === null ? "default" : "url";
  if (key === "off" || key === "0") {
    console.info("frame-latency: off (url)");
    return;
  }
  var n = key === null ? DEFAULT_LIMIT : Math.floor(Number(key));
  if (!(n >= 1 && n <= 8)) {
    console.warn("frame-latency: ignoring frame_latency=" + key + " (1 to 8, or off); using " + DEFAULT_LIMIT);
    n = DEFAULT_LIMIT;
    source = "default";
  }
  if (!window.GPUQueue || !window.requestAnimationFrame) return;
  var queue = null, frame = null, inflight = 0, deferrals = 0, nextId = -1;
  var waiting = [], byId = new Map();
  var submit = GPUQueue.prototype.submit;
  // The frame counts as in flight from its first submit, not from the end of its callback: Bevy asks
  // for its next animation frame after submitting, inside the same callback, and a count taken only
  // at the callback's end let that request through, so every other frame overran the limit by one
  // and frames ran in back-to-back pairs once the GPU was the bottleneck (docs/perf/frame-pacing/).
  GPUQueue.prototype.submit = function (buffers) {
    queue = this;
    var r = submit.call(this, buffers);
    if (frame && !frame.counted) {
      frame.counted = true;
      inflight++;
    }
    return r;
  };
  var raf = window.requestAnimationFrame.bind(window);
  var caf = window.cancelAnimationFrame.bind(window);
  function finished() {
    inflight--;
    while (inflight < n && waiting.length) {
      var e = waiting.shift();
      e.real = raf(e.run);
    }
  }
  // A callback that submitted counts as one frame in flight until the queue reports all its work done.
  function counted(cb) {
    return function (ts) {
      var f = (frame = { counted: false });
      try {
        cb(ts);
      } finally {
        frame = null;
        if (f.counted) queue.onSubmittedWorkDone().then(finished, finished);
      }
    };
  }
  window.requestAnimationFrame = function (cb) {
    if (inflight < n) return raf(counted(cb));
    deferrals++;
    var id = nextId--;
    var e = { real: 0, run: null };
    var inner = counted(cb);
    e.run = function (ts) {
      byId.delete(id);
      inner(ts);
    };
    byId.set(id, e);
    waiting.push(e);
    return id;
  };
  window.cancelAnimationFrame = function (id) {
    if (!(id < 0)) return caf(id);
    var e = byId.get(id);
    if (!e) return;
    byId.delete(id);
    var i = waiting.indexOf(e);
    if (i >= 0) waiting.splice(i, 1);
    else if (e.real) caf(e.real);
  };
  window.__frameLatency = {
    limit: n,
    source: source,
    inflight: function () { return inflight; },
    deferrals: function () { return deferrals; },
  };
  console.info("frame-latency: limit " + n + " (" + source + ")");
})();

// Frame rate cap (owner A29: Off, 30, 60 or 120 fps, Settings > Graphics). The engine owns the
// choice (`set_frame_cap`, `state().frame_cap`) and hands it here through `__frameCap.set(fps)`,
// 0 for Off. While a cap is set, every animation-frame callback on the page (the engine's, the
// HUD's, any probe's) is held until the interval since the last open frame has elapsed: one
// decision per animation frame, so every callback in an open frame runs with the same timestamp.
// It wraps the frame-latency limit above, so a held frame has not submitted and is not counted in
// flight. Off passes every request straight through.
(function () {
  if (!window.requestAnimationFrame) return;
  var raf = window.requestAnimationFrame.bind(window);
  var caf = window.cancelAnimationFrame.bind(window);
  var fps = 0, interval = 0, next = null, skipped = 0, opened = 0;
  // Held callbacks in request order, by the id handed out (a range of their own below the
  // limiter's negative ids, so either layer's cancel finds its own).
  var pending = [], byId = new Map(), nextId = -0x40000000, ticking = false;
  // A frame may open this early: rAF timestamps sit on the display's vsync grid, so a 60 cap on a
  // 60 Hz display must not wait for a timestamp exactly one interval on.
  function slack() { return Math.min(2, interval * 0.1); }
  function open(ts) {
    if (!interval) return true;
    if (next !== null && ts < next - slack()) return false;
    // The deadline moves by whole intervals; after a stall it restarts from now, with no burst.
    next = next === null || ts - next >= interval ? ts + interval : next + interval;
    return true;
  }
  function tick(ts) {
    ticking = false;
    if (!pending.length) return;
    if (!open(ts)) {
      skipped++;
      ticking = true;
      raf(tick);
      return;
    }
    opened++;
    var run = pending;
    pending = [];
    for (var i = 0; i < run.length; i++) {
      byId.delete(run[i].id);
      try {
        run[i].cb(ts);
      } catch (e) {
        setTimeout(function () { throw e; });
      }
    }
  }
  window.requestAnimationFrame = function (cb) {
    if (!interval && !pending.length) return raf(cb);
    var e = { id: nextId--, cb: cb };
    pending.push(e);
    byId.set(e.id, e);
    if (!ticking) {
      ticking = true;
      raf(tick);
    }
    return e.id;
  };
  window.cancelAnimationFrame = function (id) {
    var e = byId.get(id);
    if (!e) return caf(id);
    byId.delete(id);
    var i = pending.indexOf(e);
    if (i >= 0) pending.splice(i, 1);
  };
  window.__frameCap = {
    set: function (n) {
      n = Math.floor(Number(n));
      fps = n > 0 ? n : 0;
      interval = fps ? 1000 / fps : 0;
      next = null;
      console.info("frame-cap: " + (fps ? fps + " fps" : "off"));
    },
    fps: function () { return fps; },
    skipped: function () { return skipped; },
    opened: function () { return opened; },
  };
})();
