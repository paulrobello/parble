//! Agent operability (P0-T6): command-line flags (native) and URL query switches (web), the state
//! dump, a timed screenshot and auto-exit. See `docs/agent-operability.md`.
//!
//! All timing here is real (wall) time in the app layer. None of it reaches `parble-sim`.

use bevy::{
    prelude::*,
    render::view::screenshot::{Screenshot, ScreenshotCaptured},
};
use parble_render::{
    Quality,
    camera::{CameraDiag, CameraMode, Cues},
    post::{PostOverrides, parse_tonemapping},
};
use parble_sim::{Sim, hash::format_hash};
use serde::Serialize;

use crate::SimClock;

/// `window.__parble.ready` turns true once the sim has ticked and this many frames have run.
const READY_FRAMES: u64 = 3;
/// How long past `--exit-after` the app waits for a requested screenshot before failing.
const SHOT_GRACE_S: f64 = 10.0;
/// Exit codes: bad flags, screenshot missing or not lit.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
const EXIT_BAD_FLAGS: u8 = 2;
const EXIT_SHOT_FAILED: u8 = 3;

/// Scene to load (`--scenario`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Scenario {
    /// The P0 ramp tray with one standard marble, restarted every 10 s.
    #[default]
    Ramp,
}

impl Scenario {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "ramp" => Some(Self::Ramp),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Ramp => "ramp",
        }
    }

    pub fn build(self) -> Sim {
        match self {
            Self::Ramp => Sim::ramp_scene(),
        }
    }
}

/// Camera at startup (`--camera`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CameraChoice {
    /// The follow-marble director with the scenario's cues.
    #[default]
    Director,
    /// The editor orbit camera.
    Orbit,
    /// One fixed overview of the whole tray (the P0 view).
    Overview,
}

impl CameraChoice {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "director" => Some(Self::Director),
            "orbit" => Some(Self::Orbit),
            "overview" => Some(Self::Overview),
            _ => None,
        }
    }

    pub fn mode(self) -> CameraMode {
        match self {
            Self::Orbit => CameraMode::Orbit,
            Self::Director | Self::Overview => CameraMode::Director,
        }
    }

    pub fn cues(self, scenario: Scenario) -> Cues {
        match (self, scenario) {
            (Self::Overview, _) => Cues::overview(),
            (_, Scenario::Ramp) => Cues::ramp(),
        }
    }
}

/// Parsed agent switches. Native: `--key value` or `--key=value`; web: `?key=value&...`.
#[derive(Resource, Debug, Clone, PartialEq)]
pub struct AgentOpts {
    pub screenshot: Option<String>,
    pub screenshot_after: f64,
    pub exit_after: Option<f64>,
    pub scenario: Scenario,
    /// Recorded and reported; no P0 scenario consumes it yet (procgen arrives in P3).
    pub seed: u64,
    pub quality: Quality,
    pub dump_state: bool,
    pub camera: CameraChoice,
    /// Post overrides (`--taa`, `--dof`, `--aperture`, `--tonemap`) for look-dev and the shots test.
    pub post: PostOverrides,
    /// Lockstep: hold the sim at tick 0 for this many real seconds (pipelines compile), then advance
    /// exactly [`crate::LOCKSTEP_TICKS`] per frame, so a frame index maps to one sim state.
    pub lockstep_after: Option<f64>,
    /// Capture the screenshot on the first frame at or past this sim tick (instead of by time).
    pub screenshot_tick: Option<u64>,
}

impl Default for AgentOpts {
    fn default() -> Self {
        Self {
            screenshot: None,
            screenshot_after: 3.0,
            exit_after: None,
            scenario: Scenario::Ramp,
            seed: 0,
            quality: Quality::Medium,
            dump_state: false,
            camera: CameraChoice::Director,
            post: PostOverrides::default(),
            lockstep_after: None,
            screenshot_tick: None,
        }
    }
}

fn parse_seconds(key: &str, v: &str) -> Result<f64, String> {
    match v.parse::<f64>() {
        Ok(s) if s.is_finite() && s >= 0.0 => Ok(s),
        _ => Err(format!("{key}: expected seconds >= 0, got {v:?}")),
    }
}

fn parse_bool(key: &str, v: Option<&str>) -> Result<bool, String> {
    match v {
        None | Some("" | "1" | "true") => Ok(true),
        Some("0" | "false") => Ok(false),
        Some(v) => Err(format!("{key}: expected 1/true/0/false, got {v:?}")),
    }
}

impl AgentOpts {
    /// Apply `(key, value)` pairs. `strict` rejects unknown keys (argv); the URL query ignores them.
    pub fn from_pairs(
        pairs: impl IntoIterator<Item = (String, Option<String>)>,
        strict: bool,
    ) -> Result<Self, String> {
        let mut o = Self::default();
        for (key, value) in pairs {
            let need = |v: Option<String>| v.ok_or_else(|| format!("{key}: missing value"));
            match key.as_str() {
                "screenshot" => o.screenshot = Some(need(value)?),
                "screenshot-after" => o.screenshot_after = parse_seconds(&key, &need(value)?)?,
                "exit-after" => o.exit_after = Some(parse_seconds(&key, &need(value)?)?),
                "scenario" => {
                    let v = need(value)?;
                    o.scenario = Scenario::parse(&v)
                        .ok_or_else(|| format!("scenario: unknown {v:?} (known: ramp)"))?;
                }
                "seed" => {
                    let v = need(value)?;
                    o.seed = v
                        .parse()
                        .map_err(|_| format!("seed: expected a u64, got {v:?}"))?;
                }
                "quality" => {
                    let v = need(value)?;
                    o.quality = Quality::parse(&v)
                        .ok_or_else(|| format!("quality: unknown {v:?} (low|medium|high|ultra)"))?;
                }
                "camera" => {
                    let v = need(value)?;
                    o.camera = CameraChoice::parse(&v).ok_or_else(|| {
                        format!("camera: unknown {v:?} (director|orbit|overview)")
                    })?;
                }
                "taa" => o.post.taa = Some(parse_bool(&key, value.as_deref())?),
                "dof" => o.post.dof = Some(parse_bool(&key, value.as_deref())?),
                "aperture" => {
                    let v = need(value)?;
                    o.post.aperture = Some(
                        v.parse::<f32>()
                            .ok()
                            .filter(|f| f.is_finite() && *f > 0.0)
                            .ok_or_else(|| format!("aperture: expected f-stops > 0, got {v:?}"))?,
                    );
                }
                "tonemap" => {
                    let v = need(value)?;
                    o.post.tonemapping = Some(parse_tonemapping(&v).ok_or_else(|| {
                        format!("tonemap: unknown {v:?} (tony|agx|aces|filmic|sbdt|reinhard)")
                    })?);
                }
                "lockstep-after" => o.lockstep_after = Some(parse_seconds(&key, &need(value)?)?),
                "screenshot-tick" => {
                    let v = need(value)?;
                    o.screenshot_tick = Some(
                        v.parse()
                            .map_err(|_| format!("screenshot-tick: expected a u64, got {v:?}"))?,
                    );
                }
                "dump-state" => o.dump_state = parse_bool(&key, value.as_deref())?,
                _ if strict => return Err(format!("unknown flag --{key}")),
                _ => {}
            }
        }
        Ok(o)
    }

    /// Native argv without the program name.
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    pub fn from_args(args: impl IntoIterator<Item = String>) -> Result<Self, String> {
        let mut pairs = Vec::new();
        let mut args = args.into_iter();
        while let Some(arg) = args.next() {
            let Some(flag) = arg.strip_prefix("--") else {
                return Err(format!("unexpected argument {arg:?}"));
            };
            if let Some((k, v)) = flag.split_once('=') {
                pairs.push((k.to_owned(), Some(v.to_owned())));
            } else if flag == "dump-state" {
                pairs.push((flag.to_owned(), None));
            } else {
                pairs.push((flag.to_owned(), args.next()));
            }
        }
        Self::from_pairs(pairs, true)
    }

    /// A `location.search` string; `decode` undoes percent-encoding.
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    pub fn from_query(search: &str, decode: impl Fn(&str) -> String) -> Result<Self, String> {
        let pairs = search
            .trim_start_matches('?')
            .split('&')
            .filter(|p| !p.is_empty())
            .map(|p| {
                let p = p.replace('+', " ");
                match p.split_once('=') {
                    Some((k, v)) => (decode(k), Some(decode(v))),
                    None => (decode(&p), None),
                }
            });
        Self::from_pairs(pairs, false)
    }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "state", rename_all = "camelCase")]
enum ShotState {
    None,
    Pending,
    Capturing,
    Done { lit: f32, colors: usize, ok: bool },
    Failed { error: String },
}

#[derive(Resource, Debug)]
struct AgentRuntime {
    frames: u64,
    fps: f32,
    shot: ShotState,
    /// The camera state on the screenshot frame.
    shot_camera: Option<CameraDiag>,
    exiting: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct MarbleDiag {
    position: [f32; 3],
    rotation: [f32; 4],
    linvel: [f32; 3],
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct TrackRunDiag {
    ticks: u32,
    hash: String,
}

/// The `--dump-state` JSON and the `window.__parble` object.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DiagState<'a> {
    ready: bool,
    tick: u64,
    /// Completed 10 s runs since start.
    run: u32,
    frame: u64,
    fps: f32,
    /// Trajectory hash of the current run so far, 16 hex digits.
    hash: String,
    /// Hash of the last completed run; equals the golden `ramp_10s` hash for the ramp scenario.
    last_run_hash: Option<String>,
    marbles: Vec<MarbleDiag>,
    scenario: &'static str,
    /// Metadata name of the `.parble` track loaded over the bridge; it replaces the scenario.
    track: Option<String>,
    /// The last bridge `runTrack`: its tick count and trajectory hash.
    track_run: Option<TrackRunDiag>,
    seed: u64,
    quality: &'static str,
    screenshot: &'a ShotState,
    camera: &'a CameraDiag,
    shot_camera: Option<&'a CameraDiag>,
    exited: bool,
}

fn diag<'a>(
    clock: &SimClock,
    rt: &'a AgentRuntime,
    opts: &AgentOpts,
    camera: &'a CameraDiag,
) -> DiagState<'a> {
    let tick = clock.sim.tick_count();
    DiagState {
        ready: tick > 0 && rt.frames >= READY_FRAMES,
        tick,
        run: clock.runs,
        frame: rt.frames,
        fps: rt.fps,
        hash: format_hash(clock.sim.trajectory_hash()),
        last_run_hash: clock.last_run_hash.map(format_hash),
        marbles: clock
            .sim
            .marbles()
            .into_iter()
            .map(|m| MarbleDiag {
                position: m.position,
                rotation: m.rotation,
                linvel: m.linvel,
            })
            .collect(),
        scenario: opts.scenario.name(),
        track: clock
            .tracks
            .loaded
            .as_ref()
            .map(|f| f.metadata.name.clone()),
        track_run: clock.tracks.last_run.map(|(ticks, hash)| TrackRunDiag {
            ticks,
            hash: format_hash(hash),
        }),
        seed: opts.seed,
        quality: opts.quality.name(),
        screenshot: &rt.shot,
        camera,
        shot_camera: rt.shot_camera.as_ref(),
        exited: rt.exiting,
    }
}

/// Read the switches for this platform. Native: a bad flag prints the error and exits 2.
pub fn read_opts() -> AgentOpts {
    #[cfg(not(target_arch = "wasm32"))]
    {
        AgentOpts::from_args(std::env::args().skip(1)).unwrap_or_else(|e| {
            eprintln!("parble: {e}");
            std::process::exit(i32::from(EXIT_BAD_FLAGS));
        })
    }
    #[cfg(target_arch = "wasm32")]
    {
        use js_sys::{JsString, Reflect};
        let search = Reflect::get(&js_sys::global(), &JsString::from("location"))
            .and_then(|l| Reflect::get(&l, &JsString::from("search")))
            .ok()
            .and_then(|s| s.as_string())
            .unwrap_or_default();
        let decode = |s: &str| {
            js_sys::decode_uri_component(s)
                .map(String::from)
                .unwrap_or_else(|_| s.to_owned())
        };
        AgentOpts::from_query(&search, decode).unwrap_or_else(|e| {
            error!("parble: {e}; using defaults");
            AgentOpts::default()
        })
    }
}

pub struct AgentPlugin(pub AgentOpts);

impl Plugin for AgentPlugin {
    fn build(&self, app: &mut App) {
        let opts = self.0.clone();
        if opts.screenshot.is_some() || opts.exit_after.is_some() {
            // An agent-launched window may never get focus; unfocused throttling would stall the timers.
            app.insert_resource(bevy::winit::WinitSettings {
                focused_mode: bevy::winit::UpdateMode::Continuous,
                unfocused_mode: bevy::winit::UpdateMode::Continuous,
            });
        }
        app.insert_resource(AgentRuntime {
            frames: 0,
            fps: 0.0,
            shot: if opts.screenshot.is_some() {
                ShotState::Pending
            } else {
                ShotState::None
            },
            shot_camera: None,
            exiting: false,
        })
        .insert_resource(opts.quality)
        .insert_resource(opts)
        .add_systems(
            Last,
            (count_frames, request_shot, auto_exit, publish).chain(),
        );
    }
}

fn count_frames(time: Res<Time<Real>>, mut rt: ResMut<AgentRuntime>) {
    rt.frames += 1;
    let dt = time.delta_secs();
    if dt > 0.0 {
        let inst = 1.0 / dt;
        rt.fps = if rt.fps == 0.0 {
            inst
        } else {
            rt.fps + 0.1 * (inst - rt.fps)
        };
    }
}

fn request_shot(
    mut commands: Commands,
    time: Res<Time<Real>>,
    opts: Res<AgentOpts>,
    clock: Res<SimClock>,
    camera: Res<CameraDiag>,
    mut rt: ResMut<AgentRuntime>,
) {
    if rt.shot != ShotState::Pending {
        return;
    }
    let due = match opts.screenshot_tick {
        Some(t) => clock.sim.tick_count() >= t,
        None => time.elapsed_secs_f64() >= opts.screenshot_after,
    };
    if !due {
        return;
    }
    rt.shot = ShotState::Capturing;
    rt.shot_camera = Some(camera.clone());
    let mut shot = commands.spawn(Screenshot::primary_window());
    #[cfg(target_arch = "wasm32")]
    if let Some(path) = &opts.screenshot {
        shot.observe(bevy::render::view::screenshot::save_to_disk(path.clone()));
    }
    shot.observe(on_shot);
}

/// Lit-scene check with the web smoke's thresholds: the backdrop alone is non-black, so it needs
/// > 50 % non-black pixels, > 2 % lit pixels (max channel > 60) and > 16 distinct 4-bit colours.
fn shot_stats(rgb: &[[u8; 3]]) -> (f32, f32, usize) {
    let mut non_black = 0usize;
    let mut lit = 0usize;
    let mut colors = std::collections::BTreeSet::new();
    for p in rgb {
        let m = p[0].max(p[1]).max(p[2]);
        non_black += usize::from(m > 8);
        lit += usize::from(m > 60);
        colors.insert(
            (u16::from(p[0] >> 4) << 8) | (u16::from(p[1] >> 4) << 4) | u16::from(p[2] >> 4),
        );
    }
    let n = rgb.len().max(1) as f32;
    (non_black as f32 / n, lit as f32 / n, colors.len())
}

fn shot_ok(non_black: f32, lit: f32, colors: usize) -> bool {
    non_black > 0.5 && lit > 0.02 && colors > 16
}

fn on_shot(ev: On<ScreenshotCaptured>, opts: Res<AgentOpts>, mut rt: ResMut<AgentRuntime>) {
    let rgb = match ev.image.clone().try_into_dynamic() {
        Ok(img) => img.to_rgb8(),
        Err(e) => {
            rt.shot = ShotState::Failed {
                error: format!("{e}"),
            };
            return;
        }
    };
    let px: Vec<[u8; 3]> = rgb.pixels().map(|p| p.0).collect();
    let (non_black, lit, colors) = shot_stats(&px);
    let ok = shot_ok(non_black, lit, colors);
    #[cfg(not(target_arch = "wasm32"))]
    if let Some(path) = &opts.screenshot {
        let path = std::path::Path::new(path);
        if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Err(e) = rgb.save(path) {
            rt.shot = ShotState::Failed {
                error: format!("{}: {e}", path.display()),
            };
            return;
        }
    }
    info!(
        "screenshot {}: {}x{} nonBlack={non_black:.3} lit={lit:.3} colors={colors} ok={ok}",
        opts.screenshot.as_deref().unwrap_or(""),
        rgb.width(),
        rgb.height()
    );
    rt.shot = ShotState::Done { lit, colors, ok };
}

fn auto_exit(
    time: Res<Time<Real>>,
    opts: Res<AgentOpts>,
    clock: Res<SimClock>,
    camera: Res<CameraDiag>,
    mut rt: ResMut<AgentRuntime>,
    mut exit: MessageWriter<AppExit>,
) {
    let Some(after) = opts.exit_after else {
        return;
    };
    let t = time.elapsed_secs_f64();
    // A tick-timed screenshot exits as soon as it is written.
    let shot_settled = opts.screenshot_tick.is_some()
        && matches!(rt.shot, ShotState::Done { .. } | ShotState::Failed { .. });
    if rt.exiting || (t < after && !shot_settled) {
        return;
    }
    let waiting = matches!(rt.shot, ShotState::Pending | ShotState::Capturing);
    if waiting && t < after + SHOT_GRACE_S {
        return;
    }
    let code = match &rt.shot {
        ShotState::None | ShotState::Done { ok: true, .. } => 0,
        ShotState::Done { ok: false, .. } => {
            error!("screenshot is not a lit scene");
            EXIT_SHOT_FAILED
        }
        ShotState::Failed { error } => {
            error!("screenshot failed: {error}");
            EXIT_SHOT_FAILED
        }
        ShotState::Pending | ShotState::Capturing => {
            error!("screenshot never arrived");
            EXIT_SHOT_FAILED
        }
    };
    rt.exiting = true;
    if opts.dump_state {
        let json = serde_json::to_string(&diag(&clock, &rt, &opts, &camera)).unwrap_or_default();
        #[cfg(not(target_arch = "wasm32"))]
        println!("{json}");
        #[cfg(target_arch = "wasm32")]
        info!("parble state: {json}");
    }
    exit.write(AppExit::from_code(code));
}

#[cfg(target_arch = "wasm32")]
fn publish(
    clock: Res<SimClock>,
    rt: Res<AgentRuntime>,
    opts: Res<AgentOpts>,
    camera: Res<CameraDiag>,
) {
    use js_sys::{JSON, JsString, Reflect};
    let Ok(json) = serde_json::to_string(&diag(&clock, &rt, &opts, &camera)) else {
        return;
    };
    if let Ok(v) = JSON::parse(&json) {
        let _ = Reflect::set(&js_sys::global(), &JsString::from("__parble"), &v);
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn publish() {}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(s: &str) -> Result<AgentOpts, String> {
        AgentOpts::from_args(s.split_whitespace().map(String::from))
    }

    #[test]
    fn no_flags_give_defaults() {
        assert_eq!(args(""), Ok(AgentOpts::default()));
    }

    #[test]
    fn every_flag_parses_in_both_spellings() {
        let o = args(
            "--screenshot out/a.png --screenshot-after=1.5 --exit-after 4 --scenario ramp \
             --seed=42 --quality low --dump-state",
        )
        .unwrap();
        assert_eq!(o.screenshot.as_deref(), Some("out/a.png"));
        assert_eq!(o.screenshot_after, 1.5);
        assert_eq!(o.exit_after, Some(4.0));
        assert_eq!(o.scenario, Scenario::Ramp);
        assert_eq!(o.seed, 42);
        assert_eq!(o.quality, Quality::Low);
        assert!(o.dump_state);
    }

    #[test]
    fn bad_flags_are_rejected() {
        assert!(args("--scenario loop").is_err());
        assert!(args("--quality extreme").is_err());
        assert!(args("--camera drone").is_err());
        assert!(args("--aperture 0").is_err());
        assert!(args("--tonemap neon").is_err());
        assert!(args("--seed -1").is_err());
        assert!(args("--exit-after -2").is_err());
        assert!(args("--exit-after").is_err());
        assert!(args("--frobnicate 1").is_err());
        assert!(args("stray").is_err());
    }

    #[test]
    fn camera_post_and_lockstep_flags_parse() {
        let o = args(
            "--quality ultra --camera orbit --taa 0 --dof=1 --aperture 4 --tonemap agx \
             --lockstep-after 3 --screenshot-tick 960",
        )
        .unwrap();
        assert_eq!(o.quality, Quality::Ultra);
        assert_eq!(o.camera, CameraChoice::Orbit);
        assert_eq!(o.post.taa, Some(false));
        assert_eq!(o.post.dof, Some(true));
        assert_eq!(o.post.aperture, Some(4.0));
        assert!(o.post.tonemapping.is_some());
        assert_eq!(o.lockstep_after, Some(3.0));
        assert_eq!(o.screenshot_tick, Some(960));
        let q =
            AgentOpts::from_query("?quality=ultra&camera=overview&taa=0", str::to_owned).unwrap();
        assert_eq!(q.quality, Quality::Ultra);
        assert_eq!(q.camera, CameraChoice::Overview);
        assert_eq!(q.post.taa, Some(false));
    }

    #[test]
    fn query_matches_argv_and_ignores_unknown_keys() {
        let q = AgentOpts::from_query(
            "?screenshot=a%2Fb.png&exit-after=4&seed=7&quality=high&dump-state&utm=x",
            |s| s.replace("%2F", "/"),
        )
        .unwrap();
        assert_eq!(q.screenshot.as_deref(), Some("a/b.png"));
        assert_eq!(q.exit_after, Some(4.0));
        assert_eq!(q.seed, 7);
        assert_eq!(q.quality, Quality::High);
        assert!(q.dump_state);
        assert!(
            !AgentOpts::from_query("?dump-state=0", str::to_owned)
                .unwrap()
                .dump_state
        );
        assert!(AgentOpts::from_query("?scenario=nope", str::to_owned).is_err());
    }

    #[test]
    fn backdrop_alone_is_not_a_lit_shot() {
        let backdrop = vec![[28u8, 26, 23]; 1000];
        let (nb, lit, colors) = shot_stats(&backdrop);
        assert!(nb > 0.99 && lit == 0.0 && colors == 1);
        assert!(!shot_ok(nb, lit, colors));
        let scene: Vec<[u8; 3]> = (0..1000u32)
            .map(|i| [(i % 256) as u8, (i * 7 % 256) as u8, 120])
            .collect();
        let (nb, lit, colors) = shot_stats(&scene);
        assert!(shot_ok(nb, lit, colors));
    }
}
