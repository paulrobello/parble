//! Agent operability (P0-T6): command-line flags (native) and URL query switches (web), the state
//! dump, a timed screenshot and auto-exit. See `docs/agent-operability.md`.
//!
//! All timing here is real (wall) time in the app layer. None of it reaches `parble-sim`.

use bevy::{
    prelude::*,
    render::view::screenshot::{Screenshot, ScreenshotCaptured},
};
use parble_render::{
    Environment, PlayroomView, Quality, ShadowLights,
    camera::{CameraDiag, CameraMode, Cues},
    kit::{KitDiag, KitScene, TextureCap, family::TextureFamily, parts::PartDiag},
    marble::MarbleLook,
    post::{PostOverrides, parse_tonemapping},
    sensors::{SensorFrame, SensorOverlay},
};
use parble_sim::{
    Sim,
    hash::format_hash,
    marble::{self, MarbleKind, MarbleSize},
    track_run::TrackRun,
};
use serde::Serialize;

use crate::SimClock;
use crate::editor::EditorDiag;
use crate::playback::Playback;

/// Looks of the all-pieces marbles in release order, repeated: glass types only, since the run
/// releases glass marbles.
const ALLPIECES_LOOKS: [MarbleKind; 5] = [
    MarbleKind::Swirl,
    MarbleKind::Opal,
    MarbleKind::Onionskin,
    MarbleKind::CatsEye,
    MarbleKind::Glow,
];

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
    /// Look-dev close-up (P1-T6): one marble at rest on the tray floor under a close camera.
    Closeup,
    /// The night playroom (P1-T5) with a marble run of shipped kit pieces on the rug in place of
    /// the tray: [`parble_sim::playroom_run`], drawn by the kit renderer and simulated on chunk
    /// colliders, with its own committed hash. Ignores `--marble`.
    Playroom,
    /// The P1-T4 kit showcase: 2,000 instanced pieces in one flat, raised grid with half-cell gaps
    /// beside the ramp. Render-only, so the sim and its golden hash are the ramp's.
    Kit2000,
    /// Marble lineup (P5-T1): every type side by side at rest on the floor in front of the tray,
    /// with a mini and a shooter at the ends ([`marble::LINEUP`]). Ignores `--marble`.
    Lineup,
    /// The in-engine editor (P3-T4): the tray, no marble, an orbit camera and kit pieces edited in
    /// place ([`crate::editor`]). Never restarts, so edits keep their incremental colliders.
    Editor,
    /// Mechanism showcase (P2-T6): a toggle lane and a one-way gate lane behind the tray, one cat's
    /// eye each, the parts drawn at the sim's angles ([`parble_sim::mech_showcase`]).
    Mechanisms,
    /// All-pieces test track (P2-T7): every PRD P2 kit item behind the tray, 10 marbles released
    /// from the hopper into two finish cups ([`parble_sim::allpieces`]), with its own committed
    /// hash. Ignores `--marble`.
    AllPieces,
}

impl Scenario {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "ramp" => Some(Self::Ramp),
            "closeup" => Some(Self::Closeup),
            "playroom" => Some(Self::Playroom),
            "kit2000" => Some(Self::Kit2000),
            "lineup" => Some(Self::Lineup),
            "editor" => Some(Self::Editor),
            "mechanisms" => Some(Self::Mechanisms),
            "allpieces" => Some(Self::AllPieces),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Ramp => "ramp",
            Self::Closeup => "closeup",
            Self::Playroom => "playroom",
            Self::Kit2000 => "kit2000",
            Self::Lineup => "lineup",
            Self::Editor => "editor",
            Self::Mechanisms => "mechanisms",
            Self::AllPieces => "allpieces",
        }
    }

    /// The scenario's run. No `--marble` at the standard size keeps the P0 legacy marble, so
    /// `ramp` and `playroom` stay the golden run; otherwise the type's physical table (P1-T6) at
    /// the chosen size (P5-T1, [`marble::spec_for`]). `allpieces` is a track run whose hopper
    /// releases its marbles as it goes.
    pub fn build(self, marble: Option<MarbleKind>, size: MarbleSize) -> TrackRun {
        let spec = marble::spec_for(marble, size);
        TrackRun::from_sim(match self {
            Self::Ramp | Self::Kit2000 => Sim::ramp_scene_with(spec),
            Self::Playroom => Sim::playroom_scene(),
            Self::Closeup => Sim::closeup_scene(spec),
            Self::Lineup => Sim::lineup_scene(),
            Self::Editor => Sim::tray_scene(),
            Self::Mechanisms => Sim::mech_showcase_scene(),
            Self::AllPieces => {
                return parble_sim::allpieces::run().expect("the all-pieces track runs");
            }
        })
    }

    /// The marble looks in sim spawn order: the lineup's list, or the one chosen marble (the legacy
    /// marble renders as a cat's eye).
    pub fn looks(self, marble: Option<MarbleKind>, size: MarbleSize) -> Vec<(MarbleKind, f32)> {
        match self {
            Self::Lineup => marble::LINEUP
                .iter()
                .map(|&(k, s)| (k, s.radius_m()))
                .collect(),
            Self::Playroom => parble_sim::playroom_run::MARBLES
                .iter()
                .map(|&(k, s)| (k, s.radius_m()))
                .collect(),
            Self::Mechanisms => {
                vec![(MarbleKind::CatsEye, parble_sim::marble::CATS_EYE.radius_m); 2]
            }
            Self::AllPieces => ALLPIECES_LOOKS
                .iter()
                .cycle()
                .take(parble_sim::allpieces::MARBLES)
                .map(|&k| (k, MarbleSize::Standard.radius_m()))
                .collect(),
            _ => vec![(
                marble.unwrap_or_default(),
                marble::spec_for(marble, size).radius_m,
            )],
        }
    }

    pub fn environment(self) -> Environment {
        match self {
            Self::Ramp
            | Self::Closeup
            | Self::Kit2000
            | Self::Lineup
            | Self::Editor
            | Self::Mechanisms
            | Self::AllPieces => Environment::Plain,
            Self::Playroom => Environment::Playroom,
        }
    }

    pub fn kit(self) -> KitScene {
        match self {
            Self::Kit2000 => KitScene::Stress2000,
            Self::Playroom => KitScene::Playroom,
            Self::Editor => KitScene::Editor,
            Self::Mechanisms => KitScene::Mechanisms,
            Self::AllPieces => KitScene::AllPieces,
            _ => KitScene::None,
        }
    }

    /// Ticks in one run before the scenario restarts: 10 s, the playroom run's 4 s, or the
    /// all-pieces run's 30 s.
    pub fn run_ticks(self) -> u32 {
        match self {
            Self::Playroom => parble_sim::playroom_run::PLAYROOM_TICKS,
            Self::AllPieces => parble_sim::allpieces::RUN_TICKS,
            Self::Editor => u32::MAX,
            _ => parble_sim::golden::RAMP_10S_TICKS,
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

    /// Director cues. A playroom `--view` is one fixed shot; without it the playroom follows the
    /// lead marble round its kit run.
    pub fn cues(self, scenario: Scenario, view: Option<PlayroomView>) -> Cues {
        match (self, scenario, view) {
            (Self::Overview, ..) => Cues::overview(),
            (_, Scenario::Kit2000, _) => Cues::kit(),
            (_, Scenario::Playroom, Some(v)) => Cues::playroom(v),
            (_, Scenario::Playroom, None) => Cues::playroom_run(),
            (_, Scenario::Ramp | Scenario::Editor, _) => Cues::ramp(),
            (_, Scenario::Closeup, _) => Cues::closeup(),
            (_, Scenario::Lineup, _) => Cues::lineup(),
            (_, Scenario::Mechanisms, _) => Cues::mechanisms(),
            (_, Scenario::AllPieces, _) => Cues::allpieces(),
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
    /// Marble type (`--marble`). `None` is the P0 legacy marble (golden physics, cat's eye look).
    pub marble: Option<MarbleKind>,
    /// Marble size (`--marble-size`, P5-T1). Standard without `--marble` is the legacy marble; any
    /// other size without `--marble` is a cat's eye at that size.
    pub marble_size: MarbleSize,
    pub dump_state: bool,
    pub camera: CameraChoice,
    /// Post overrides (`--taa`, `--dof`, `--aperture`, `--tonemap`) for look-dev and the shots test.
    pub post: PostOverrides,
    /// Lockstep: hold the sim at tick 0 for this many real seconds (pipelines compile), then advance
    /// exactly [`crate::LOCKSTEP_TICKS`] per frame, so a frame index maps to one sim state.
    pub lockstep_after: Option<f64>,
    /// Capture the screenshot on the first frame at or past this sim tick (instead of by time).
    pub screenshot_tick: Option<u64>,
    /// Playroom fixed framing (`--view rug|blocks`) for the director; ignored by other scenarios.
    pub view: Option<PlayroomView>,
    /// Texture family cap (`--tex bc7|astc|png`) for the kit material; unset picks by device.
    pub tex: Option<TextureFamily>,
    /// Editor only (`--editor-fill <n>`): start with an `n`-piece cell grid behind the tray, laid as
    /// one undoable paste (the 2,000-piece edit perf track).
    pub editor_fill: Option<u32>,
    /// Start with the mechanism sensor overlay on (`--sensors`, `?sensors=1`); `G` toggles it.
    pub sensors: bool,
    /// Starting time scale (`--time-scale`, one of [`crate::playback::TIME_SCALES`]); `,` and `.`
    /// change it.
    pub time_scale: f64,
    /// Start paused (`--paused`, `?paused=1`); `P` toggles it.
    pub paused: bool,
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
            marble: None,
            marble_size: MarbleSize::Standard,
            dump_state: false,
            camera: CameraChoice::Director,
            post: PostOverrides::default(),
            lockstep_after: None,
            screenshot_tick: None,
            view: None,
            tex: None,
            editor_fill: None,
            sensors: false,
            time_scale: 1.0,
            paused: false,
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
                    o.scenario = Scenario::parse(&v).ok_or_else(|| {
                        format!("scenario: unknown {v:?} (known: ramp, closeup, playroom, kit2000, lineup, editor, mechanisms, allpieces)")
                    })?;
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
                "bloom" => o.post.bloom = Some(parse_bool(&key, value.as_deref())?),
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
                "marble" => {
                    let v = need(value)?;
                    o.marble =
                        Some(MarbleKind::parse(&v).ok_or_else(|| {
                            format!("marble: unknown {v:?} ({})", MarbleKind::NAMES)
                        })?);
                }
                "marble-size" => {
                    let v = need(value)?;
                    o.marble_size = MarbleSize::parse(&v).ok_or_else(|| {
                        format!("marble-size: unknown {v:?} ({})", MarbleSize::NAMES)
                    })?;
                }
                "dump-state" => o.dump_state = parse_bool(&key, value.as_deref())?,
                "sensors" => o.sensors = parse_bool(&key, value.as_deref())?,
                "paused" => o.paused = parse_bool(&key, value.as_deref())?,
                "time-scale" | "timescale" => {
                    let v = need(value)?;
                    o.time_scale = v
                        .parse::<f64>()
                        .ok()
                        .filter(|s| crate::playback::level_of(*s).is_some())
                        .ok_or_else(|| format!("{key}: expected 1, 0.5, 0.25 or 0.1, got {v:?}"))?;
                }
                "view" => {
                    let v = need(value)?;
                    o.view = Some(
                        PlayroomView::parse(&v)
                            .ok_or_else(|| format!("view: unknown {v:?} (rug|blocks)"))?,
                    );
                }
                "tex" => {
                    let v = need(value)?;
                    o.tex = Some(
                        TextureFamily::parse(&v)
                            .ok_or_else(|| format!("tex: unknown {v:?} (bc7|astc|png)"))?,
                    );
                }
                "editor-fill" => {
                    let v = need(value)?;
                    o.editor_fill = Some(
                        v.parse::<u32>()
                            .ok()
                            .filter(|n| *n <= crate::editor::MAX_FILL)
                            .ok_or_else(|| {
                                format!(
                                    "editor-fill: expected 0..={} pieces, got {v:?}",
                                    crate::editor::MAX_FILL
                                )
                            })?,
                    );
                }
                _ if strict => return Err(format!("unknown flag --{key}")),
                _ => {}
            }
        }
        if o.post.dof.is_none()
            && matches!(
                o.scenario,
                Scenario::Kit2000 | Scenario::Mechanisms | Scenario::AllPieces
            )
        {
            // Owner direction 2026-11-08: the piece showcases are judged sharp, with no depth of field.
            o.post.dof = Some(false);
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
            } else if matches!(flag, "dump-state" | "sensors" | "paused") {
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
    /// The mechanism part poses on the screenshot frame (`kit.parts`).
    shot_parts: Option<Vec<PartDiag>>,
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
    /// Marble type; `"legacy"` is the P0 marble the golden run uses.
    marble: &'static str,
    /// Marble size (P5-T1): `mini`, `standard` or `shooter`.
    marble_size: &'static str,
    /// Lights casting shadows this frame, and the tier's cap (at most 3 on High).
    shadow_lights: usize,
    shadow_light_cap: usize,
    view: Option<&'static str>,
    /// The P1-T4 kit renderer (pieces, chunks, draw counts, texture family); absent without a kit.
    kit: Option<&'a KitDiag>,
    /// The editor (P3-T4); only in the `editor` scenario.
    editor: Option<&'a EditorDiag>,
    screenshot: &'a ShotState,
    camera: &'a CameraDiag,
    shot_camera: Option<&'a CameraDiag>,
    #[serde(skip_serializing_if = "Option::is_none")]
    shot_parts: Option<&'a Vec<PartDiag>>,
    /// The mechanism sensor overlay: whether it is drawn, and the sensors and occupied sensors.
    sensors: SensorsDiag,
    /// Pause and slow motion ([`crate::playback`]).
    playback: PlaybackDiag,
    exited: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PlaybackDiag {
    paused: bool,
    /// The chosen time scale; it is kept while paused.
    time_scale: f64,
}

#[derive(Serialize, Default)]
struct SensorsDiag {
    on: bool,
    volumes: usize,
    occupied: usize,
}

#[allow(clippy::too_many_arguments)]
fn diag<'a>(
    clock: &SimClock,
    playback: &Playback,
    rt: &'a AgentRuntime,
    opts: &AgentOpts,
    camera: &'a CameraDiag,
    shadows: &ShadowLights,
    kit: &'a KitDiag,
    editor: &'a EditorDiag,
) -> DiagState<'a> {
    let tick = clock.sim.tick_count();
    DiagState {
        // Started paused, the sim never ticks on its own.
        ready: (tick > 0 || playback.paused) && rt.frames >= READY_FRAMES,
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
        marble: match (opts.scenario, opts.marble, opts.marble_size) {
            (Scenario::Lineup, ..) => "lineup",
            (Scenario::Playroom, ..) => "playroom",
            (Scenario::Mechanisms, ..) => "mechanisms",
            (Scenario::AllPieces, ..) => "allpieces",
            (_, None, MarbleSize::Standard) => "legacy",
            (_, m, _) => m.unwrap_or_default().name(),
        },
        marble_size: opts.marble_size.name(),
        shadow_lights: shadows.casting,
        shadow_light_cap: shadows.cap,
        view: opts.view.map(PlayroomView::name),
        kit: (opts.scenario.kit() != KitScene::None).then_some(kit),
        editor: (opts.scenario == Scenario::Editor).then_some(editor),
        screenshot: &rt.shot,
        camera,
        shot_camera: rt.shot_camera.as_ref(),
        shot_parts: rt.shot_parts.as_ref(),
        sensors: SensorsDiag::default(),
        playback: PlaybackDiag {
            paused: playback.paused,
            time_scale: playback.scale(),
        },
        exited: rt.exiting,
    }
}

fn sensors_diag(overlay: &SensorOverlay, frame: &SensorFrame) -> SensorsDiag {
    SensorsDiag {
        on: overlay.0,
        volumes: frame.0.len(),
        occupied: frame.0.iter().filter(|s| s.occupied).count(),
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

/// The site's content-hash map (`window.__PARBLE_HASHES`, inlined by scripts/web/version-dist.mjs),
/// site path → hash, for versioning the wasm's asset fetches. Empty natively or when absent.
pub fn asset_versions() -> std::collections::HashMap<String, String> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::collections::HashMap::new()
    }
    #[cfg(target_arch = "wasm32")]
    {
        use js_sys::{Array, JsString, Object, Reflect};
        use wasm_bindgen::JsCast;
        let Ok(map) = Reflect::get(&js_sys::global(), &JsString::from("__PARBLE_HASHES")) else {
            return Default::default();
        };
        let Some(map) = map.dyn_ref::<Object>() else {
            return Default::default();
        };
        Object::entries(map)
            .iter()
            .filter_map(|e| {
                let e: Array = e.into();
                Some((e.get(0).as_string()?, e.get(1).as_string()?))
            })
            .collect()
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
            shot_parts: None,
            exiting: false,
        })
        .insert_resource(opts.quality)
        .insert_resource(MarbleLook(
            opts.scenario.looks(opts.marble, opts.marble_size),
        ))
        .insert_resource(opts.scenario.environment())
        .insert_resource(opts.scenario.kit())
        .insert_resource(TextureCap(opts.tex))
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
    kit: Res<KitDiag>,
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
    rt.shot_parts = (!kit.parts.is_empty()).then(|| kit.parts.clone());
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

#[allow(clippy::too_many_arguments)]
fn auto_exit(
    time: Res<Time<Real>>,
    opts: Res<AgentOpts>,
    clock: Res<SimClock>,
    camera: Res<CameraDiag>,
    shadows: Res<ShadowLights>,
    kit: Res<KitDiag>,
    editor: Res<EditorDiag>,
    sensor_overlay: Res<SensorOverlay>,
    sensor_frame: Res<SensorFrame>,
    playback: Res<Playback>,
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
        let mut state = diag(
            &clock, &playback, &rt, &opts, &camera, &shadows, &kit, &editor,
        );
        state.sensors = sensors_diag(&sensor_overlay, &sensor_frame);
        let json = serde_json::to_string(&state).unwrap_or_default();
        #[cfg(not(target_arch = "wasm32"))]
        println!("{json}");
        #[cfg(target_arch = "wasm32")]
        info!("parble state: {json}");
    }
    exit.write(AppExit::from_code(code));
}

#[cfg(target_arch = "wasm32")]
#[allow(clippy::too_many_arguments)]
fn publish(
    clock: Res<SimClock>,
    rt: Res<AgentRuntime>,
    opts: Res<AgentOpts>,
    camera: Res<CameraDiag>,
    shadows: Res<ShadowLights>,
    kit: Res<KitDiag>,
    editor: Res<EditorDiag>,
    sensor_overlay: Res<SensorOverlay>,
    sensor_frame: Res<SensorFrame>,
    playback: Res<Playback>,
) {
    use js_sys::{JSON, JsString, Reflect};
    let mut state = diag(
        &clock, &playback, &rt, &opts, &camera, &shadows, &kit, &editor,
    );
    state.sensors = sensors_diag(&sensor_overlay, &sensor_frame);
    let Ok(json) = serde_json::to_string(&state) else {
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
             --seed=42 --quality low --marble steel --dump-state --sensors --paused \
             --time-scale 0.25",
        )
        .unwrap();
        assert_eq!(o.screenshot.as_deref(), Some("out/a.png"));
        assert_eq!(o.screenshot_after, 1.5);
        assert_eq!(o.exit_after, Some(4.0));
        assert_eq!(o.scenario, Scenario::Ramp);
        assert_eq!(o.seed, 42);
        assert_eq!(o.quality, Quality::Low);
        assert_eq!(o.marble, Some(MarbleKind::Steel));
        assert!(o.dump_state);
        assert!(o.sensors);
        assert!(o.paused);
        assert_eq!(o.time_scale, 0.25);
        assert!(!args("--paused=0").unwrap().paused);
        assert!(args("--paused --scenario editor").unwrap().paused);
        assert_eq!(args("--timescale=0.1").unwrap().time_scale, 0.1);
        assert!(!args("--sensors=0").unwrap().sensors);
        assert!(args("--sensors=1 --scenario mechanisms").unwrap().sensors);
    }

    #[test]
    fn bad_flags_are_rejected() {
        assert!(args("--scenario loop").is_err());
        assert!(args("--quality extreme").is_err());
        assert!(args("--camera drone").is_err());
        assert!(args("--aperture 0").is_err());
        assert!(args("--tonemap neon").is_err());
        assert!(args("--marble agate").is_err());
        assert!(args("--marble-size huge").is_err());
        assert!(args("--seed -1").is_err());
        assert!(args("--exit-after -2").is_err());
        assert!(args("--exit-after").is_err());
        assert!(args("--frobnicate 1").is_err());
        assert!(args("--view sky").is_err());
        assert!(args("stray").is_err());
        assert!(args("--time-scale 0.3").is_err());
        assert!(args("--time-scale 2").is_err());
        assert!(args("--time-scale").is_err());
        assert!(args("--paused maybe").is_err());
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
    fn playroom_scenario_parses_and_runs_its_kit_run() {
        let o = args("--scenario playroom --view blocks").unwrap();
        assert_eq!(o.scenario, Scenario::Playroom);
        assert_eq!(o.view, Some(PlayroomView::Blocks));
        assert_eq!(
            o.camera.cues(o.scenario, o.view),
            Cues::playroom(PlayroomView::Blocks)
        );
        assert_eq!(
            CameraChoice::Director.cues(Scenario::Playroom, None),
            Cues::playroom_run()
        );
        assert_eq!(o.scenario.environment(), Environment::Playroom);
        assert_eq!(o.scenario.kit(), KitScene::Playroom);
        let q = AgentOpts::from_query("?scenario=playroom&view=rug", str::to_owned).unwrap();
        assert_eq!(
            (q.scenario, q.view),
            (Scenario::Playroom, Some(PlayroomView::Rug))
        );
        // `--marble` does not reach the playroom run: one look per run marble, in spawn order.
        let mut sim = Scenario::Playroom.build(Some(MarbleKind::Steel), MarbleSize::Shooter);
        let looks = Scenario::Playroom.looks(Some(MarbleKind::Steel), MarbleSize::Shooter);
        assert_eq!(looks.len(), sim.marbles().len());
        assert_eq!(
            Scenario::Playroom.run_ticks(),
            parble_sim::playroom_run::PLAYROOM_TICKS
        );
        sim.tick(Scenario::Playroom.run_ticks());
        assert_eq!(
            sim.trajectory_hash(),
            parble_sim::playroom_run::playroom_4s_hash()
        );
    }

    #[test]
    fn mechanisms_scenario_parses_and_runs_its_mechanisms() {
        let o = args("--scenario mechanisms").unwrap();
        assert_eq!(o.scenario, Scenario::Mechanisms);
        assert_eq!(
            Scenario::parse(o.scenario.name()),
            Some(Scenario::Mechanisms)
        );
        assert_eq!(o.scenario.kit(), KitScene::Mechanisms);
        assert_eq!(o.scenario.environment(), Environment::Plain);
        let sim = o.scenario.build(None, MarbleSize::Standard);
        let looks = o.scenario.looks(None, MarbleSize::Standard);
        assert_eq!(looks.len(), sim.marbles().len());
        let kinds: Vec<_> = sim.mechanisms().iter().map(|m| m.kind).collect();
        assert_eq!(kinds, ["toggle", "gate_oneway"]);
    }

    #[test]
    fn showcases_default_to_no_depth_of_field() {
        for s in ["kit2000", "mechanisms", "allpieces"] {
            assert_eq!(
                args(&format!("--scenario {s}")).unwrap().post.dof,
                Some(false),
                "{s}"
            );
            assert_eq!(
                args(&format!("--scenario {s} --dof 1")).unwrap().post.dof,
                Some(true),
                "{s}"
            );
        }
        assert_eq!(args("--scenario playroom").unwrap().post.dof, None);
    }

    #[test]
    fn allpieces_scenario_parses_and_runs_the_golden_track() {
        let o = args("--scenario allpieces").unwrap();
        assert_eq!(o.scenario, Scenario::AllPieces);
        assert_eq!(
            Scenario::parse(o.scenario.name()),
            Some(Scenario::AllPieces)
        );
        let q = AgentOpts::from_query("?scenario=allpieces", str::to_owned).unwrap();
        assert_eq!(q.scenario, Scenario::AllPieces);
        assert_eq!(o.scenario.kit(), KitScene::AllPieces);
        assert_eq!(o.scenario.environment(), Environment::Plain);
        assert_eq!(o.camera.cues(o.scenario, None), Cues::allpieces());
        let looks = o.scenario.looks(None, MarbleSize::Standard);
        assert_eq!(looks.len(), parble_sim::allpieces::MARBLES);
        let mut sim = o.scenario.build(None, MarbleSize::Standard);
        sim.tick(o.scenario.run_ticks());
        assert_eq!(sim.marbles().len(), parble_sim::allpieces::MARBLES);
        let golden = include_str!("../../parble-sim/tests/golden/allpieces_30s.hash");
        assert_eq!(format!("{:016x}", sim.trajectory_hash()), golden.trim());
    }

    #[test]
    fn kit2000_scenario_and_tex_cap_parse_and_keep_the_ramp_golden() {
        let o = args("--scenario kit2000 --tex astc").unwrap();
        assert_eq!(o.scenario, Scenario::Kit2000);
        assert_eq!(o.scenario.kit(), KitScene::Stress2000);
        assert_eq!(Scenario::Ramp.kit(), KitScene::None);
        assert_eq!(o.tex, Some(TextureFamily::Astc));
        assert_eq!(o.camera.cues(o.scenario, None), Cues::kit());
        assert_eq!(o.scenario.environment(), Environment::Plain);
        assert!(args("--tex etc2").is_err());
        let q = AgentOpts::from_query("?scenario=kit2000&tex=png", str::to_owned).unwrap();
        assert_eq!(
            (q.scenario, q.tex),
            (Scenario::Kit2000, Some(TextureFamily::Png))
        );
        let mut sim = Scenario::Kit2000.build(None, MarbleSize::Standard);
        sim.tick(parble_sim::golden::RAMP_10S_TICKS);
        assert_eq!(sim.trajectory_hash(), parble_sim::golden::ramp_10s_hash());
    }

    #[test]
    fn marble_size_and_lineup_parse_and_keep_the_golden_default() {
        let o = args("--marble opal --marble-size shooter").unwrap();
        assert_eq!(o.marble, Some(MarbleKind::Opal));
        assert_eq!(o.marble_size, MarbleSize::Shooter);
        let q = AgentOpts::from_query("?marble=glow&marble-size=mini", str::to_owned).unwrap();
        assert_eq!(
            (q.marble, q.marble_size),
            (Some(MarbleKind::Glow), MarbleSize::Mini)
        );
        // No flags and an explicit standard size both stay the P0 golden run.
        for o in [args("").unwrap(), args("--marble-size standard").unwrap()] {
            let mut sim = o.scenario.build(o.marble, o.marble_size);
            sim.tick(parble_sim::golden::RAMP_10S_TICKS);
            assert_eq!(sim.trajectory_hash(), parble_sim::golden::ramp_10s_hash());
        }
        let sim = Scenario::Ramp.build(Some(MarbleKind::Swirl), MarbleSize::Shooter);
        assert_eq!(sim.marbles().len(), 1);
        let lineup = args("--scenario lineup").unwrap();
        assert_eq!(lineup.scenario, Scenario::Lineup);
        let looks = lineup.scenario.looks(None, MarbleSize::Standard);
        assert_eq!(looks.len(), marble::LINEUP.len());
        assert_eq!(
            Scenario::Lineup
                .build(None, MarbleSize::Standard)
                .marbles()
                .len(),
            looks.len()
        );
        for k in MarbleKind::ALL {
            assert!(
                looks.iter().any(|(l, _)| *l == k),
                "{k:?} missing from the lineup"
            );
        }
        assert_eq!(
            Scenario::Ramp.looks(None, MarbleSize::Mini),
            vec![(MarbleKind::CatsEye, MarbleSize::Mini.radius_m())]
        );
    }

    #[test]
    fn query_matches_argv_and_ignores_unknown_keys() {
        let q = AgentOpts::from_query(
            "?screenshot=a%2Fb.png&exit-after=4&seed=7&quality=high&marble=cats-eye&dump-state&sensors=1&paused=1&timescale=0.5&utm=x",
            |s| s.replace("%2F", "/"),
        )
        .unwrap();
        assert_eq!(q.screenshot.as_deref(), Some("a/b.png"));
        assert_eq!(q.exit_after, Some(4.0));
        assert_eq!(q.seed, 7);
        assert_eq!(q.quality, Quality::High);
        assert_eq!(q.marble, Some(MarbleKind::CatsEye));
        assert!(q.dump_state);
        assert!(q.sensors);
        assert!(q.paused);
        assert_eq!(q.time_scale, 0.5);
        assert_eq!(
            AgentOpts::from_query("?time-scale=0.25", str::to_owned)
                .unwrap()
                .time_scale,
            0.25
        );
        assert!(AgentOpts::from_query("?timescale=3", str::to_owned).is_err());
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
