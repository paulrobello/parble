//! In-engine editor interaction (P3-T4), active in the `editor` scenario.
//!
//! - Track: a [`parble_core::Editor`] (P3-T1 commands, undo and redo) over the builtin kit
//!   catalogue. The palette is every builtin kit piece, in catalogue id order ([`kit::PIECES`]);
//!   the brush starts on `blk_straight_l3`.
//! - Incremental rebuild: after every command, undo or redo the editor diffs the placements by
//!   anchor, removes and places only the changed ones in the live sim's chunk colliders, and hands
//!   the renderer the new content of only the chunks those anchors sit in. Dirty colliders are
//!   rebuilt in stages (a dense chunk is about 17 ms of work natively, more in wasm), as many as
//!   fit a per-frame budget ([`COLLIDER_BUDGET_MS`]), so one chunk spreads over a few frames.
//! - Picking: a voxel walk of the camera ray through the occupied cells (pieces above the height
//!   slice are skipped), else the ground plane at the slice level ([`pick`]).
//! - Ghost: the brush at the picked cell, valid or invalid by [`Editor::check`], with one hint per
//!   port: linked, open, or a fault against the piece across.
//! - Actions: `window.wasmBindings.parbleEdit(json)` on the web queues one action for the next
//!   frame; the mouse and keyboard produce the same actions. State is `window.__parble.editor`.
//!   See `docs/agent-operability.md` (Editor).

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::Mutex;

use bevy::{platform::time::Instant, prelude::*, window::PrimaryWindow};
use parble_core::{
    CellCoord, EditCommand, Editor, HopperConfig, IssueKind, LinkFault, MarbleSize, Mirror,
    Orientation, PieceCatalog, PieceId, Placement, PortFace, PortFlow, ReleaseMode, Selection,
    Track, Yaw4, file::ParbleFile, is_finish, is_hopper, link_fault, validate_track,
};
use parble_kit::KitCatalog;
use parble_render::{
    MarbleFrame,
    camera::{CameraMode, EditorCamera, MainCamera, Orbit},
    kit::{
        self, CELL_M,
        edit::{EditPiece, KitEdits, KitGhost, KitMarks, MAX_MARKS, PortHint, chunk_of_cell},
        parts::PartFrame,
    },
};
use parble_sim::track_run::TrackRun;
use serde::{Deserialize, Serialize};

use crate::SimClock;
use crate::bridge::{self, Event};
use crate::playback::Playback;

/// Largest `fill` (and `--editor-fill`).
pub const MAX_FILL: u32 = 4_000;
/// Collider rebuild time per frame. A rebuild stage that starts inside it always finishes, so a
/// frame's rebuild is at most this plus one stage (gather, simplify, tag or install of one chunk).
pub const COLLIDER_BUDGET_MS: f32 = 4.0;
/// Fill grid: columns along +x from cell x 0, rows along −z from cell z −5.
const FILL_COLS: i32 = 50;
const FILL_Z0: i32 = -5;
/// Anchors listed in the diag at most (the count is always exact).
const MAX_LISTED: usize = 64;
/// Cursor travel (px) that turns a press into a drag.
const DRAG_PX: f32 = 4.0;
/// Most marbles a hopper queue holds (`marbles`), and the queue an empty one runs with.
pub const MAX_MARBLES: u32 = 10;
/// Release interval of an `interval` run without `intervalTicks`: 1 s.
const DEFAULT_INTERVAL_TICKS: u32 = 240;
/// Issues listed in the diag at most (`issueCount` is always exact).
const MAX_ISSUES: usize = 32;
/// The name of a track that was not loaded from a file.
const UNTITLED: &str = "Untitled track";
/// Support pieces, counted apart from their `blk` family in `selectionInfo.families`.
const SUPPORTS: [&str; 2] = ["blk_pillar", "blk_tower_socket_l1"];

/// Editor tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Tool {
    #[default]
    Place,
    Select,
    Delete,
}

/// One editor action (`parbleEdit` JSON, tagged by `op`). Pixel coordinates are CSS pixels from
/// the canvas's top-left, like `Window::cursor_position`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "op", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Action {
    Tool {
        tool: Tool,
    },
    /// The brush: a palette piece id, quarter turns and mirror.
    Brush {
        piece: String,
        #[serde(default)]
        yaw: u8,
        #[serde(default)]
        mirror: bool,
    },
    /// Pin the hover to a pixel until the real cursor moves.
    Hover {
        x: f32,
        y: f32,
    },
    /// Pin the place target to a cell (no picking).
    HoverCell {
        cell: [i32; 3],
    },
    Unhover {},
    /// The current tool at a pixel: place the brush, select (shift adds) or delete.
    Click {
        x: f32,
        y: f32,
        #[serde(default)]
        shift: bool,
    },
    Place {
        piece: String,
        cell: [i32; 3],
        #[serde(default)]
        yaw: u8,
        #[serde(default)]
        mirror: bool,
    },
    /// Select every piece with a footprint cell in the inclusive cell box.
    BoxSelect {
        a: [i32; 3],
        b: [i32; 3],
        #[serde(default)]
        shift: bool,
    },
    /// Select every visible piece whose cell centre projects inside the pixel rectangle.
    BoxSelectScreen {
        x0: f32,
        y0: f32,
        x1: f32,
        y1: f32,
        #[serde(default)]
        shift: bool,
    },
    /// Move-group drag from pixel to pixel: the selection (or the piece under the start, if it is
    /// not selected) moves by the cell offset between the two points on its level's plane.
    Drag {
        x0: f32,
        y0: f32,
        x1: f32,
        y1: f32,
    },
    Move {
        offset: [i32; 3],
    },
    /// Rotate the brush (place tool) or the selection (other tools) by quarter turns.
    Rotate {
        #[serde(default = "one")]
        by: u8,
    },
    Mirror {},
    Remove {},
    ClearSelection {},
    Undo {},
    Redo {},
    /// Show only levels at most `level`; `null` shows all.
    Slice {
        level: Option<i32>,
    },
    Camera {
        target: [f32; 3],
        yaw: f32,
        pitch: f32,
        radius: f32,
    },
    /// Pixels of cells: each cell's centre and its bottom-face centre (where a ground pick lands).
    Project {
        cells: Vec<[i32; 3]>,
    },
    /// Lay an `count`-piece grid behind the tray as one undoable paste.
    Fill {
        count: u32,
    },
    /// Remove every piece (one undoable command).
    Clear {},
    /// The editor's keyboard shortcuts on or off; the mouse is unaffected.
    Keys {
        enabled: bool,
    },
    /// Queue `count` standard marbles in the hopper (one undoable command).
    Marbles {
        count: u32,
    },
    /// Run the track from tick 0: the hopper releases its queue (`one`, `all`, `interval`,
    /// `sequential`).
    Run {
        release: String,
        #[serde(default)]
        interval_ticks: Option<u32>,
    },
    /// Back to the idle track: no marbles.
    Stop {},
    /// The active run again from tick 0, or the idle track.
    Reset {},
    /// Pause or resume the sim; no `paused` toggles.
    Pause {
        #[serde(default)]
        paused: Option<bool>,
    },
    /// The follow camera (director) on, or the orbit camera.
    Follow {
        on: bool,
    },
    Rename {
        name: String,
    },
    /// Push a `trackExported` bridge event with the track as `.parble` text.
    Export {},
}

fn one() -> u8 {
    1
}

#[derive(Debug, Deserialize)]
struct Envelope {
    #[serde(default)]
    id: Option<u32>,
    #[serde(flatten)]
    action: Action,
}

/// Queued actions. A `Mutex`, not a thread-local: natively the systems run on worker threads.
static INBOX: Mutex<VecDeque<(Option<u32>, Action)>> = Mutex::new(VecDeque::new());

fn inbox() -> std::sync::MutexGuard<'static, VecDeque<(Option<u32>, Action)>> {
    INBOX
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Parses and queues one action for the next frame.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub fn post_action(json: &str) -> Result<(), String> {
    let e: Envelope = serde_json::from_str(json).map_err(|e| format!("bad editor action: {e}"))?;
    inbox().push_back((e.id, e.action));
    Ok(())
}

#[cfg(target_arch = "wasm32")]
mod web {
    use wasm_bindgen::prelude::*;

    /// `window.wasmBindings.parbleEdit(json)`: queue one editor action. Throws on bad JSON.
    #[wasm_bindgen(js_name = parbleEdit)]
    pub fn parble_edit(json: &str) -> Result<(), JsError> {
        super::post_action(json).map_err(|e| JsError::new(&e))
    }
}

/// A camera ray hit: a piece cell (with the face normal the ray entered through) or the ground
/// plane cell at the slice level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hit {
    Piece {
        cell: CellCoord,
        anchor: CellCoord,
        normal: CellCoord,
    },
    Ground {
        cell: CellCoord,
    },
}

impl Hit {
    /// Where the brush would go: the empty cell in front of a hit face, or the ground cell.
    pub fn target(self) -> Option<CellCoord> {
        match self {
            Self::Piece { cell, normal, .. } if normal != CellCoord::ORIGIN => Some(cell + normal),
            Self::Piece { .. } => None,
            Self::Ground { cell } => Some(cell),
        }
    }
}

/// Cell-space position: cell `(i, j, k)` spans `[i, i + 1)` on each axis (a cell's origin is its
/// bottom-face centre, kit contract §1).
fn to_cell_space(p: Vec3) -> Vec3 {
    p / CELL_M + Vec3::new(0.5, 0.0, 0.5)
}

fn floor_cell(u: Vec3) -> CellCoord {
    CellCoord::new(u.x.floor() as i32, u.y.floor() as i32, u.z.floor() as i32)
}

/// The cell on horizontal plane `level` (its bottom) that the ray crosses, if it does.
pub fn plane_cell(origin: Vec3, dir: Vec3, level: i32) -> Option<CellCoord> {
    let (o, d) = (to_cell_space(origin), dir / CELL_M);
    if d.y.abs() < 1e-9 {
        return None;
    }
    let t = (level as f32 - o.y) / d.y;
    (t > 0.0).then(|| {
        let u = o + d * t;
        CellCoord::new(u.x.floor() as i32, level, u.z.floor() as i32)
    })
}

/// Walk the ray (world metres) through the cell grid. `occupant` maps a cell to the anchor of
/// the visible piece covering it. The ground is the plane at the bottom of `level`, hit from above.
pub fn pick(
    origin: Vec3,
    dir: Vec3,
    level: i32,
    occupant: impl Fn(CellCoord) -> Option<CellCoord>,
) -> Option<Hit> {
    const MAX_STEPS: usize = 4_096;
    let o = to_cell_space(origin);
    let d = dir.normalize_or_zero() / CELL_M;
    if d == Vec3::ZERO {
        return None;
    }
    let t_ground = if d.y < 0.0 && o.y > level as f32 {
        (level as f32 - o.y) / d.y
    } else {
        f32::INFINITY
    };
    let mut cell = floor_cell(o);
    let step = [d.x, d.y, d.z].map(|v| if v > 0.0 { 1 } else { -1 });
    let pos = [o.x, o.y, o.z];
    let dv = [d.x, d.y, d.z];
    let mut t_max = [0f32; 3];
    let mut t_delta = [0f32; 3];
    for a in 0..3 {
        if dv[a] == 0.0 {
            t_max[a] = f32::INFINITY;
            t_delta[a] = f32::INFINITY;
        } else {
            let c = [cell.x, cell.y, cell.z][a] as f32;
            let next = if dv[a] > 0.0 { c + 1.0 } else { c };
            t_max[a] = (next - pos[a]) / dv[a];
            t_delta[a] = 1.0 / dv[a].abs();
        }
    }
    let mut t = 0.0f32;
    let mut normal = CellCoord::ORIGIN;
    for _ in 0..MAX_STEPS {
        if t > t_ground {
            let u = o + d * t_ground;
            return Some(Hit::Ground {
                cell: CellCoord::new(u.x.floor() as i32, level, u.z.floor() as i32),
            });
        }
        if let Some(anchor) = occupant(cell) {
            return Some(Hit::Piece {
                cell,
                anchor,
                normal,
            });
        }
        let a = if t_max[0] < t_max[1] && t_max[0] < t_max[2] {
            0
        } else if t_max[1] < t_max[2] {
            1
        } else {
            2
        };
        if !t_max[a].is_finite() {
            break;
        }
        t = t_max[a];
        t_max[a] += t_delta[a];
        let mut n = [0; 3];
        n[a] = -step[a];
        normal = CellCoord::new(n[0], n[1], n[2]);
        match a {
            0 => cell.x += step[0],
            1 => cell.y += step[1],
            _ => cell.z += step[2],
        }
    }
    t_ground.is_finite().then(|| {
        let u = o + d * t_ground;
        Hit::Ground {
            cell: CellCoord::new(u.x.floor() as i32, level, u.z.floor() as i32),
        }
    })
}

fn cell_arr(c: CellCoord) -> [i32; 3] {
    [c.x, c.y, c.z]
}

fn arr_cell(a: [i32; 3]) -> CellCoord {
    CellCoord::new(a[0], a[1], a[2])
}

fn ivec(c: CellCoord) -> IVec3 {
    IVec3::new(c.x, c.y, c.z)
}

/// Bottom-face centre of a cell, metres.
fn cell_floor_m(c: CellCoord) -> Vec3 {
    kit::cell_origin(ivec(c))
}

/// One ghost port: its world cell, face and hint.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PortDiag {
    pub cell: [i32; 3],
    pub face: PortFace,
    pub hint: PortHint,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HoverDiag {
    /// `cursor`, `pixel` (a pinned `hover`) or `cell` (a pinned `hoverCell`).
    pub source: &'static str,
    /// `piece`, `ground`, or `null` for a pinned cell or a miss.
    pub hit: Option<&'static str>,
    pub hit_cell: Option<[i32; 3]>,
    /// Anchor of the piece under the cursor.
    pub hit_anchor: Option<[i32; 3]>,
    /// Where the brush would go.
    pub target: Option<[i32; 3]>,
    pub valid: bool,
    pub error: Option<String>,
    pub ports: Vec<PortDiag>,
}

/// Timing of the last edit, ms. `collider_ms` and `collider_chunks` sum every frame's budgeted
/// rebuild until the edit's dirty chunks are done.
#[derive(Debug, Clone, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct EditTiming {
    pub op: &'static str,
    /// Command, diff, collider insert/remove and render hand-off.
    pub edit_ms: f32,
    pub changed: usize,
    pub render_chunks: usize,
    pub collider_ms: f32,
    pub collider_chunks: usize,
    /// Frames the collider rebuild spread over.
    pub collider_frames: usize,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LastAction {
    pub id: Option<u32>,
    pub op: &'static str,
    pub ok: bool,
    pub error: Option<String>,
    /// `project` results: `[centreX, centreY, floorX, floorY]` per cell (CSS px), or `null`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub projected: Option<Vec<Option<[f32; 4]>>>,
}

/// `window.__parble.editor`. Small on purpose: it is serialized every frame.
#[derive(Resource, Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorDiag {
    pub tool: Tool,
    pub brush: BrushDiag,
    pub palette: Vec<&'static str>,
    pub pieces: usize,
    /// Pieces at or below the slice.
    pub visible_pieces: usize,
    pub slice: Option<i32>,
    pub selection: usize,
    /// Up to 64 selected anchors.
    pub selected: Vec<[i32; 3]>,
    pub can_undo: bool,
    pub can_redo: bool,
    pub hover: Option<HoverDiag>,
    /// The live move drag's offset and validity.
    pub drag: Option<DragDiag>,
    pub collider_chunks: usize,
    pub dirty_collider_chunks: usize,
    /// Actions applied since start.
    pub seq: u64,
    pub edits: u64,
    pub last: LastAction,
    pub last_edit: Option<EditTiming>,
    pub stats: EditStats,
    /// The editor's keyboard shortcuts are on.
    pub keys: bool,
    pub name: String,
    pub run: RunDiag,
    /// Hopper and finish placements.
    pub hoppers: usize,
    pub finishes: usize,
    /// Validation of the track with its hopper resolved, as of the last edit.
    pub issue_count: usize,
    /// The first 32 issues.
    pub issues: Vec<IssueDiag>,
    /// The selection's make-up; `null` when nothing is selected.
    pub selection_info: Option<SelectionDiag>,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunDiag {
    /// A run started by `run` (or a loaded track with a hopper queue) is live.
    pub active: bool,
    pub release: &'static str,
    pub interval_ticks: u32,
    /// Marbles the hopper releases in a run.
    pub queue: usize,
    /// Marbles in the sim now.
    pub marbles: usize,
    pub tick: u64,
    pub paused: bool,
    pub follow: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct IssueDiag {
    pub kind: &'static str,
    pub piece: Option<String>,
    pub anchor: Option<[i32; 3]>,
    /// The first cell involved.
    pub cell: Option<[i32; 3]>,
    pub face: Option<PortFace>,
    /// `brokenLink` only.
    pub fault: Option<&'static str>,
    /// The other placement of an `overlap` or a `brokenLink`.
    pub other_piece: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SelectionDiag {
    pub count: usize,
    /// By id family (`blk`, `rail`, `mech`), with supports as `support`.
    pub families: BTreeMap<String, usize>,
    /// Lowest and highest anchor level.
    pub levels: [i32; 2],
    pub single: Option<SingleDiag>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SingleDiag {
    pub piece: String,
    pub anchor: [i32; 3],
    pub yaw: u8,
    pub mirror: bool,
    pub ports: Vec<SelectedPort>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SelectedPort {
    pub face: PortFace,
    pub flow: PortFlow,
    pub hint: PortHint,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrushDiag {
    pub piece: &'static str,
    pub yaw: u8,
    pub mirror: bool,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DragDiag {
    pub offset: [i32; 3],
    pub valid: bool,
}

/// Maxima over every edit since start. The `fill` that lays a track is kept apart in `fill`.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EditStats {
    pub edit_ms_max: f32,
    /// Longest single frame of budgeted collider rebuild.
    pub collider_frame_ms_max: f32,
    /// Longest single rebuild stage.
    pub collider_step_ms_max: f32,
    pub collider_chunks_total: usize,
    pub collider_ms_total: f32,
    /// The last `fill`: its timing, kept apart from the edit maxima.
    pub fill: Option<EditTiming>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum HoverSrc {
    Cursor,
    Pixel(Vec2),
    Cell(CellCoord),
}

#[derive(Debug, Clone, Copy)]
struct Brush {
    piece: usize,
    yaw: u8,
    mirror: bool,
}

/// The run the overlay asked for last.
#[derive(Debug, Clone, Copy, PartialEq)]
struct RunState {
    active: bool,
    release: ReleaseMode,
    /// The last interval asked for, kept while another mode runs.
    interval: u32,
}

impl RunState {
    fn set_release(&mut self, release: ReleaseMode) {
        self.release = release;
        if let ReleaseMode::Interval { ticks } = release {
            self.interval = ticks;
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum Press {
    Box,
    Move { anchor: CellCoord },
}

#[derive(Resource)]
struct EditState {
    editor: Editor,
    kit: &'static KitCatalog,
    tool: Tool,
    brush: Brush,
    slice: Option<i32>,
    /// Anchor → placement, mirrored from the track after every edit.
    by_anchor: BTreeMap<CellCoord, Placement>,
    /// Covered cell → anchor.
    occupancy: BTreeMap<CellCoord, CellCoord>,
    /// Render chunk → anchors in it.
    by_chunk: BTreeMap<[i32; 3], BTreeSet<CellCoord>>,
    /// Anchor → placement index.
    index_of: BTreeMap<CellCoord, u32>,
    hover_src: HoverSrc,
    last_cursor: Option<Vec2>,
    /// Ghost cache key: (target, brush, edits).
    ghost_key: Option<(Option<CellCoord>, usize, u8, bool, u64)>,
    press: Option<(Vec2, Press)>,
    dragging: bool,
    /// Edit whose dirty colliders are still being rebuilt.
    pending: Option<EditTiming>,
    pending_is_fill: bool,
    marks_key: Option<(u64, usize)>,
    /// [`SimClock::restarts`] this state is synced to.
    restarts: u64,
    /// The editor's keyboard shortcuts are on (`keys`).
    keys: bool,
    name: String,
    run: RunState,
    /// Edit count the issues and placement counts were computed at.
    checked_at: Option<u64>,
    /// Edit count and selection `selectionInfo` was computed for.
    selection_at: Option<(u64, Selection)>,
}

fn palette() -> Vec<&'static str> {
    kit::PIECES.iter().map(|(id, _)| *id).collect()
}

fn piece_index(id: &str) -> Option<usize> {
    kit::piece_index(id)
}

fn pid(i: usize) -> PieceId {
    PieceId::new(kit::PIECES[i].0).expect("kit ids follow the id grammar")
}

fn edit_piece(p: &Placement) -> Option<EditPiece> {
    Some(EditPiece {
        piece: piece_index(p.piece.as_str())?,
        cell: ivec(p.anchor),
        yaw: p.yaw.quarter_turns(),
        mirror: p.mirror == Mirror::On,
    })
}

/// A cheap deterministic hash for the fill layout.
fn mix(i: u32) -> u32 {
    let mut h = i.wrapping_mul(0x9e37_79b9) ^ 0x85eb_ca6b;
    h ^= h >> 15;
    h = h.wrapping_mul(0x2c1b_3c6d);
    h ^ (h >> 12)
}

/// The `fill` layout: `count` one-cell [`kit::SHOWCASE`] pieces in [`FILL_COLS`] columns behind
/// the tray.
pub fn fill_layout(count: u32) -> Vec<Placement> {
    (0..count)
        .map(|i| {
            let (col, row) = (i as i32 % FILL_COLS, i as i32 / FILL_COLS);
            let h = mix(i);
            Placement::new(
                pid(kit::showcase_piece(
                    (h % kit::SHOWCASE.len() as u32) as usize,
                )),
                CellCoord::new(col, 0, FILL_Z0 - row),
                Yaw4::new(((h >> 8) % 4) as u8),
                Mirror::Off,
            )
        })
        .collect()
}

impl EditState {
    fn new() -> Self {
        Self {
            editor: Editor::new(Track::new(0)),
            kit: parble_kit::builtin(),
            tool: Tool::Place,
            brush: Brush {
                piece: kit::showcase_piece(0),
                yaw: 0,
                mirror: false,
            },
            slice: None,
            by_anchor: BTreeMap::new(),
            occupancy: BTreeMap::new(),
            by_chunk: BTreeMap::new(),
            index_of: BTreeMap::new(),
            hover_src: HoverSrc::Cursor,
            last_cursor: None,
            ghost_key: None,
            press: None,
            dragging: false,
            pending: None,
            pending_is_fill: false,
            marks_key: None,
            restarts: 0,
            keys: true,
            name: UNTITLED.to_owned(),
            run: RunState {
                active: false,
                release: ReleaseMode::default(),
                interval: DEFAULT_INTERVAL_TICKS,
            },
            checked_at: None,
            selection_at: None,
        }
    }

    /// Start over on `track`, the track a bridge `loadTrack` or `runTrack` restarted the live sim
    /// on: its placements become the editor's, with an empty selection and no undo or redo
    /// history. The sim already holds their colliders, so only the maps and the render chunks
    /// change.
    fn adopt(&mut self, track: Track, edits: &mut KitEdits) {
        for ch in std::mem::take(&mut self.by_chunk).into_keys() {
            edits.pending.insert(ch, Vec::new());
        }
        self.by_anchor.clear();
        self.occupancy.clear();
        self.editor = Editor::new(track);
        let track = self.editor.track();
        self.index_of = (0u32..)
            .zip(&track.placements)
            .map(|(i, p)| (p.anchor, i))
            .collect();
        for p in &track.placements {
            if let Some(def) = self.kit.get(&p.piece) {
                for c in p.world_cells(def) {
                    self.occupancy.insert(c, p.anchor);
                }
            }
            self.by_chunk
                .entry(chunk_of_cell(ivec(p.anchor)))
                .or_default()
                .insert(p.anchor);
            self.by_anchor.insert(p.anchor, p.clone());
        }
        for (ch, set) in &self.by_chunk {
            let pieces = set
                .iter()
                .filter_map(|a| edit_piece(&self.by_anchor[a]))
                .collect();
            edits.pending.insert(*ch, pieces);
        }
        self.press = None;
        self.dragging = false;
        self.pending = None;
        self.pending_is_fill = false;
    }

    fn visible(&self, anchor: CellCoord) -> bool {
        self.slice.is_none_or(|s| anchor.y <= s)
    }

    fn occupant(&self, cell: CellCoord) -> Option<CellCoord> {
        self.occupancy
            .get(&cell)
            .copied()
            .filter(|a| self.visible(*a))
    }

    fn pick_ray(&self, ray: Ray3d) -> Option<Hit> {
        pick(
            ray.origin,
            *ray.direction,
            self.slice.unwrap_or(0).max(0),
            |c| self.occupant(c),
        )
    }

    fn placement_at(&self, target: CellCoord) -> Placement {
        Placement::new(
            pid(self.brush.piece),
            target,
            Yaw4::new(self.brush.yaw),
            if self.brush.mirror {
                Mirror::On
            } else {
                Mirror::Off
            },
        )
    }

    /// Port hints for `p` against the placed pieces.
    fn port_hints(&self, p: &Placement) -> Vec<PortDiag> {
        let Some(def) = self.kit.get(&p.piece) else {
            return Vec::new();
        };
        let o = Orientation::new(p.yaw, p.mirror);
        def.ports
            .iter()
            .map(|port| {
                let mut wp = port.transformed(o);
                wp.cell = wp.cell + p.anchor;
                let across = wp.cell + wp.face.normal();
                let hint = match self.occupancy.get(&across) {
                    None => PortHint::Open,
                    Some(anchor) => {
                        let other = &self.by_anchor[anchor];
                        let mate = self.kit.get(&other.piece).and_then(|d| {
                            let oo = Orientation::new(other.yaw, other.mirror);
                            d.ports.iter().map(|q| q.transformed(oo)).find_map(|mut q| {
                                q.cell = q.cell + other.anchor;
                                (q.cell == across && q.face == wp.face.opposite()).then_some(q)
                            })
                        });
                        match mate {
                            Some(q) if link_fault(&wp, &q).is_none() => PortHint::Linked,
                            _ => PortHint::Fault,
                        }
                    }
                };
                PortDiag {
                    cell: cell_arr(wp.cell),
                    face: wp.face,
                    hint,
                }
            })
            .collect()
    }

    /// Mirror the track into the maps and push only the changed placements to the sim and the
    /// renderer. Returns (changed placements, render chunks).
    fn sync(&mut self, clock: &mut SimClock, edits: &mut KitEdits) -> (usize, usize) {
        let track = self.editor.track();
        let new: BTreeMap<CellCoord, Placement> = track
            .placements
            .iter()
            .map(|p| (p.anchor, p.clone()))
            .collect();
        self.index_of = (0u32..)
            .zip(&track.placements)
            .map(|(i, p)| (p.anchor, i))
            .collect();
        let removed: Vec<CellCoord> = self
            .by_anchor
            .iter()
            .filter(|(a, p)| new.get(a) != Some(p))
            .map(|(a, _)| *a)
            .collect();
        let added: Vec<Placement> = new
            .iter()
            .filter(|(a, p)| self.by_anchor.get(a) != Some(p))
            .map(|(_, p)| p.clone())
            .collect();
        let mut dirty = BTreeSet::new();
        for a in &removed {
            if let Some(p) = self.by_anchor.remove(a) {
                if let Some(def) = self.kit.get(&p.piece) {
                    for c in p.world_cells(def) {
                        self.occupancy.remove(&c);
                    }
                }
                let ch = chunk_of_cell(ivec(*a));
                if let Some(set) = self.by_chunk.get_mut(&ch) {
                    set.remove(a);
                }
                dirty.insert(ch);
            }
            if let Err(e) = clock.sim.remove_piece(*a) {
                error!("editor: collider remove {a:?}: {e}");
            }
        }
        for p in &added {
            if let Some(def) = self.kit.get(&p.piece) {
                for c in p.world_cells(def) {
                    self.occupancy.insert(c, p.anchor);
                }
            }
            let ch = chunk_of_cell(ivec(p.anchor));
            self.by_chunk.entry(ch).or_default().insert(p.anchor);
            dirty.insert(ch);
            if let Err(e) = clock.sim.place_piece(self.kit, p.clone()) {
                error!("editor: collider place {:?}: {e}", p.anchor);
            }
            self.by_anchor.insert(p.anchor, p.clone());
        }
        for ch in &dirty {
            let pieces = self
                .by_chunk
                .get(ch)
                .map(|set| {
                    set.iter()
                        .filter_map(|a| edit_piece(&self.by_anchor[a]))
                        .collect()
                })
                .unwrap_or_default();
            if self.by_chunk.get(ch).is_some_and(BTreeSet::is_empty) {
                self.by_chunk.remove(ch);
            }
            edits.pending.insert(*ch, pieces);
        }
        (removed.len() + added.len(), dirty.len())
    }

    fn selection_for(&self, anchors: impl IntoIterator<Item = CellCoord>) -> Selection {
        anchors
            .into_iter()
            .filter_map(|a| self.index_of.get(&a).copied())
            .collect()
    }
}

struct Ctx<'a> {
    clock: &'a mut SimClock,
    edits: &'a mut KitEdits,
    diag: &'a mut EditorDiag,
    camera: Option<(&'a Camera, &'a GlobalTransform)>,
    editor_cam: &'a mut EditorCamera,
    frame: &'a mut MarbleFrame,
    parts: &'a mut PartFrame,
    playback: &'a mut Playback,
    mode: &'a mut CameraMode,
}

impl Ctx<'_> {
    fn ray(&self, px: Vec2) -> Option<Ray3d> {
        let (cam, gt) = self.camera?;
        cam.viewport_to_world(gt, px).ok()
    }

    fn project(&self, p: Vec3) -> Option<Vec2> {
        let (cam, gt) = self.camera?;
        cam.world_to_viewport(gt, p).ok()
    }
}

/// Apply one editor command, sync, and record its timing.
fn run_command(
    st: &mut EditState,
    ctx: &mut Ctx,
    op: &'static str,
    cmd: EditCommand,
) -> Result<(), String> {
    let start = Instant::now();
    st.editor.apply(st.kit, cmd).map_err(|e| e.to_string())?;
    finish_edit(st, ctx, op, start);
    Ok(())
}

fn finish_edit(st: &mut EditState, ctx: &mut Ctx, op: &'static str, start: Instant) {
    let (changed, render_chunks) = st.sync(ctx.clock, ctx.edits);
    let edit_ms = start.elapsed().as_secs_f32() * 1e3;
    ctx.diag.edits += 1;
    let is_fill = op == "fill";
    if !is_fill {
        ctx.diag.stats.edit_ms_max = ctx.diag.stats.edit_ms_max.max(edit_ms);
    }
    // A new edit before the last one's colliders finish folds into it.
    let mut t = st.pending.take().unwrap_or_default();
    t.op = op;
    t.edit_ms += edit_ms;
    t.changed += changed;
    t.render_chunks += render_chunks;
    st.pending = Some(t.clone());
    st.pending_is_fill = st.pending_is_fill || is_fill;
    ctx.diag.last_edit = Some(t);
}

fn parse_piece(id: &str) -> Result<usize, String> {
    piece_index(id).ok_or_else(|| format!("piece {id} is not in the palette {:?}", palette()))
}

fn mirror_of(m: bool) -> Mirror {
    if m { Mirror::On } else { Mirror::Off }
}

fn op_name(a: &Action) -> &'static str {
    match a {
        Action::Tool { .. } => "tool",
        Action::Brush { .. } => "brush",
        Action::Hover { .. } => "hover",
        Action::HoverCell { .. } => "hoverCell",
        Action::Unhover {} => "unhover",
        Action::Click { .. } => "click",
        Action::Place { .. } => "place",
        Action::BoxSelect { .. } => "boxSelect",
        Action::BoxSelectScreen { .. } => "boxSelectScreen",
        Action::Drag { .. } => "drag",
        Action::Move { .. } => "move",
        Action::Rotate { .. } => "rotate",
        Action::Mirror {} => "mirror",
        Action::Remove {} => "remove",
        Action::ClearSelection {} => "clearSelection",
        Action::Undo {} => "undo",
        Action::Redo {} => "redo",
        Action::Slice { .. } => "slice",
        Action::Camera { .. } => "camera",
        Action::Project { .. } => "project",
        Action::Fill { .. } => "fill",
        Action::Clear {} => "clear",
        Action::Keys { .. } => "keys",
        Action::Marbles { .. } => "marbles",
        Action::Run { .. } => "run",
        Action::Stop {} => "stop",
        Action::Reset {} => "reset",
        Action::Pause { .. } => "pause",
        Action::Follow { .. } => "follow",
        Action::Rename { .. } => "rename",
        Action::Export {} => "export",
    }
}

/// The track as the editor validates, runs and exports it. The hopper is the stored placement when
/// that is a hopper, else the first hopper placed (placing one never sets it); an empty queue is
/// [`MAX_MARBLES`] standard marbles.
pub fn resolve_hopper(track: &Track) -> Track {
    let mut t = track.clone();
    let stored = t.hopper.placement.filter(|&i| {
        t.placements
            .get(i as usize)
            .is_some_and(|p| is_hopper(&p.piece))
    });
    t.hopper.placement = stored.or_else(|| {
        t.placements
            .iter()
            .position(|p| is_hopper(&p.piece))
            .and_then(|i| u32::try_from(i).ok())
    });
    if t.hopper.queue.is_empty() {
        t.hopper.queue = vec![MarbleSize::Standard; MAX_MARBLES as usize];
    }
    t
}

/// Marbles a run of `hopper` releases ([`resolve_hopper`]).
fn queue_len(hopper: &HopperConfig) -> usize {
    match hopper.queue.len() {
        0 => MAX_MARBLES as usize,
        n => n,
    }
}

/// The hopper config of a `marbles` op: `count` standard marbles, the release mode kept and the
/// placement resolved, so a stale stored index never reaches the command.
fn marbles_hopper(track: &Track, count: u32) -> Result<HopperConfig, String> {
    if count == 0 || count > MAX_MARBLES {
        return Err(format!("marbles: expected 1..={MAX_MARBLES}"));
    }
    Ok(HopperConfig {
        placement: resolve_hopper(track).hopper.placement,
        queue: vec![MarbleSize::Standard; count as usize],
        release: track.hopper.release,
    })
}

/// `track`, hopper resolved, as canonical `.parble` text named `name`.
fn export_json(track: &Track, name: &str) -> Result<String, String> {
    let file = ParbleFile::new(resolve_hopper(track), name);
    file.validate().map_err(|e| format!("export: {e}"))?;
    Ok(file.to_json())
}

/// A `run` release mode by name; an interval defaults to [`DEFAULT_INTERVAL_TICKS`].
fn release_mode(name: &str, interval_ticks: Option<u32>) -> Result<ReleaseMode, String> {
    Ok(match name {
        "one" => ReleaseMode::One,
        "all" => ReleaseMode::All,
        "sequential" => ReleaseMode::Sequential,
        "interval" => match interval_ticks.unwrap_or(DEFAULT_INTERVAL_TICKS) {
            0 => return Err("run: intervalTicks must be at least 1".into()),
            ticks => ReleaseMode::Interval { ticks },
        },
        _ => {
            return Err(format!(
                "run: release {name} is not one, all, interval or sequential"
            ));
        }
    })
}

fn release_name(release: ReleaseMode) -> &'static str {
    match release {
        ReleaseMode::One => "one",
        ReleaseMode::All => "all",
        ReleaseMode::Interval { .. } => "interval",
        ReleaseMode::Sequential => "sequential",
    }
}

/// A fresh run of `track` at tick 0, recording its events: with `release`, the resolved hopper
/// releases its queue in that mode; without, the track idles with no marbles.
fn editor_run(track: &Track, release: Option<ReleaseMode>) -> Result<TrackRun, String> {
    let run_track = match release {
        Some(release) => {
            let mut t = resolve_hopper(track);
            if t.hopper.placement.is_none() {
                return Err("no hopper".into());
            }
            t.hopper.release = release;
            t
        }
        None => Track {
            hopper: HopperConfig::default(),
            ..track.clone()
        },
    };
    TrackRun::new(&run_track)
        .map(crate::recording)
        .map_err(|e| e.to_string())
}

/// Swap the live sim for a fresh run of the editor's track. The run is built whole, so no collider
/// rebuild is left for the last edit.
fn start_run(
    st: &mut EditState,
    ctx: &mut Ctx,
    release: Option<ReleaseMode>,
) -> Result<(), String> {
    let run = editor_run(st.editor.track(), release)?;
    ctx.clock.install(run, ctx.frame, ctx.parts);
    st.pending = None;
    st.pending_is_fill = false;
    st.run.active = release.is_some();
    Ok(())
}

/// A trimmed track name of 1 to [`parble_core::file::MAX_NAME_CHARS`] characters.
fn track_name(name: &str) -> Result<String, String> {
    let name = name.trim();
    let n = name.chars().count();
    if n == 0 || n > parble_core::file::MAX_NAME_CHARS {
        return Err(format!(
            "rename: expected 1..={} characters",
            parble_core::file::MAX_NAME_CHARS
        ));
    }
    Ok(name.to_owned())
}

fn fault_name(fault: &LinkFault) -> &'static str {
    match fault {
        LinkFault::NoMatingPort => "noMatingPort",
        LinkFault::LevelMismatch { .. } => "levelMismatch",
        LinkFault::ClassMismatch { .. } => "classMismatch",
        LinkFault::FlowConflict { .. } => "flowConflict",
    }
}

/// The issue count and the first [`MAX_ISSUES`] issues of `track`.
fn issue_diags(track: &Track, kit: &KitCatalog) -> (usize, Vec<IssueDiag>) {
    let issues = validate_track(track, kit);
    let placement = |i: usize| track.placements.get(i);
    let piece = |i: usize| placement(i).map(|p| p.piece.as_str().to_owned());
    let listed = issues
        .iter()
        .take(MAX_ISSUES)
        .map(|issue| {
            let (kind, face, fault, other) = match &issue.kind {
                IssueKind::OpenPort { face, .. } => ("openPort", Some(*face), None, None),
                IssueKind::BrokenLink {
                    port, other, fault, ..
                } => {
                    let face = issue.placement.and_then(placement).and_then(|p| {
                        let port = kit.get(&p.piece)?.ports.get(*port)?;
                        Some(port.transformed(Orientation::new(p.yaw, p.mirror)).face)
                    });
                    ("brokenLink", face, Some(fault_name(fault)), Some(*other))
                }
                IssueKind::Unreachable => ("unreachable", None, None, None),
                IssueKind::FinishUnreachable => ("finishUnreachable", None, None, None),
                IssueKind::MissingHopper => ("missingHopper", None, None, None),
                IssueKind::MissingFinish => ("missingFinish", None, None, None),
                IssueKind::Overlap { other } => ("overlap", None, None, Some(*other)),
                IssueKind::UnknownPiece { .. } => ("unknownPiece", None, None, None),
                IssueKind::LevelSuffixMismatch { .. } => ("levelSuffixMismatch", None, None, None),
                IssueKind::HopperConfigInvalid { .. } => ("hopperConfigInvalid", None, None, None),
            };
            IssueDiag {
                kind,
                piece: issue.placement.and_then(piece),
                anchor: issue.anchor.map(cell_arr),
                cell: issue.cells.first().copied().map(cell_arr),
                face,
                fault,
                other_piece: other.and_then(piece),
            }
        })
        .collect();
    (issues.len(), listed)
}

fn family_of(id: &PieceId) -> &str {
    if SUPPORTS.contains(&id.as_str()) {
        "support"
    } else {
        id.family()
    }
}

/// The selection's families and levels, and the one selected piece's ports.
fn selection_diag(st: &EditState) -> Option<SelectionDiag> {
    let placements = &st.editor.track().placements;
    let selected: Vec<&Placement> = st
        .editor
        .selection
        .iter()
        .filter_map(|i| placements.get(i as usize))
        .collect();
    let first = *selected.first()?;
    let mut families = BTreeMap::new();
    let mut levels = [first.anchor.y; 2];
    for p in &selected {
        *families.entry(family_of(&p.piece).to_owned()).or_insert(0) += 1;
        levels = [levels[0].min(p.anchor.y), levels[1].max(p.anchor.y)];
    }
    let single = (selected.len() == 1).then(|| {
        let o = Orientation::new(first.yaw, first.mirror);
        let ports = st
            .kit
            .get(&first.piece)
            .map(|def| {
                def.ports
                    .iter()
                    .zip(st.port_hints(first))
                    .map(|(port, h)| SelectedPort {
                        face: h.face,
                        flow: port.transformed(o).flow,
                        hint: h.hint,
                    })
                    .collect()
            })
            .unwrap_or_default();
        SingleDiag {
            piece: first.piece.as_str().to_owned(),
            anchor: cell_arr(first.anchor),
            yaw: first.yaw.quarter_turns(),
            mirror: first.mirror == Mirror::On,
            ports,
        }
    });
    Some(SelectionDiag {
        count: selected.len(),
        families,
        levels,
        single,
    })
}

/// The selection a move drag from `anchor` acts on: the selection if it holds `anchor`, else
/// that piece alone.
fn drag_selection(st: &EditState, anchor: CellCoord) -> Selection {
    let i = st.index_of.get(&anchor).copied();
    match i {
        Some(i) if st.editor.selection.contains(i) => st.editor.selection.clone(),
        Some(i) => [i].into_iter().collect(),
        None => Selection::new(),
    }
}

/// The cell offset of a move drag from `p0` to `p1` on the plane of `anchor`'s level.
fn drag_offset(ctx: &Ctx, anchor: CellCoord, p0: Vec2, p1: Vec2) -> Option<CellCoord> {
    let (r0, r1) = (ctx.ray(p0)?, ctx.ray(p1)?);
    let a = plane_cell(r0.origin, *r0.direction, anchor.y)?;
    let b = plane_cell(r1.origin, *r1.direction, anchor.y)?;
    Some(CellCoord::new(b.x - a.x, 0, b.z - a.z))
}

fn screen_select(st: &EditState, ctx: &Ctx, a: Vec2, b: Vec2) -> Selection {
    let (lo, hi) = (a.min(b), a.max(b));
    let anchors = st
        .by_anchor
        .keys()
        .filter(|&&c| st.visible(c))
        .filter(|&&c| {
            ctx.project(cell_floor_m(c) + Vec3::Y * (CELL_M * 0.5))
                .is_some_and(|p| p.cmpge(lo).all() && p.cmple(hi).all())
        })
        .copied()
        .collect::<Vec<_>>();
    st.selection_for(anchors)
}

fn union(a: &Selection, b: Selection) -> Selection {
    a.iter().chain(b.iter()).collect()
}

fn apply_action(st: &mut EditState, ctx: &mut Ctx, action: Action) -> Result<LastAction, String> {
    let mut out = LastAction::default();
    match action {
        Action::Tool { tool } => st.tool = tool,
        Action::Brush { piece, yaw, mirror } => {
            let i = parse_piece(&piece)?;
            if mirror && !st.kit.get(&pid(i)).is_some_and(|d| d.chiral) {
                return Err(format!(
                    "piece {piece} is not chiral and cannot be mirrored"
                ));
            }
            st.brush = Brush {
                piece: i,
                yaw: yaw % 4,
                mirror,
            };
        }
        Action::Hover { x, y } => st.hover_src = HoverSrc::Pixel(Vec2::new(x, y)),
        Action::HoverCell { cell } => st.hover_src = HoverSrc::Cell(arr_cell(cell)),
        Action::Unhover {} => st.hover_src = HoverSrc::Cursor,
        Action::Click { x, y, shift } => click(st, ctx, Vec2::new(x, y), shift)?,
        Action::Place {
            piece,
            cell,
            yaw,
            mirror,
        } => {
            let p = Placement::new(
                pid(parse_piece(&piece)?),
                arr_cell(cell),
                Yaw4::new(yaw % 4),
                mirror_of(mirror),
            );
            run_command(st, ctx, "place", EditCommand::Place(p))?;
        }
        Action::BoxSelect { a, b, shift } => {
            let mut sel = Selection::boxed(st.editor.track(), st.kit, arr_cell(a), arr_cell(b));
            if let Some(s) = st.slice {
                sel = sel
                    .iter()
                    .filter(|&i| st.editor.track().placements[i as usize].anchor.y <= s)
                    .collect();
            }
            st.editor.selection = if shift {
                union(&st.editor.selection, sel)
            } else {
                sel
            };
        }
        Action::BoxSelectScreen {
            x0,
            y0,
            x1,
            y1,
            shift,
        } => {
            let sel = screen_select(st, ctx, Vec2::new(x0, y0), Vec2::new(x1, y1));
            st.editor.selection = if shift {
                union(&st.editor.selection, sel)
            } else {
                sel
            };
        }
        Action::Drag { x0, y0, x1, y1 } => {
            let (p0, p1) = (Vec2::new(x0, y0), Vec2::new(x1, y1));
            let Some(Hit::Piece { anchor, .. }) = ctx.ray(p0).and_then(|r| st.pick_ray(r)) else {
                return Err("drag: no piece under the start point".into());
            };
            let selection = drag_selection(st, anchor);
            let offset = drag_offset(ctx, anchor, p0, p1).ok_or("drag: off the plane")?;
            if offset == CellCoord::ORIGIN {
                return Err("drag: no cell offset".into());
            }
            run_command(
                st,
                ctx,
                "drag",
                EditCommand::MoveGroup { selection, offset },
            )?;
        }
        Action::Move { offset } => {
            let selection = st.editor.selection.clone();
            run_command(
                st,
                ctx,
                "move",
                EditCommand::MoveGroup {
                    selection,
                    offset: arr_cell(offset),
                },
            )?;
        }
        Action::Rotate { by } => {
            if st.tool == Tool::Place {
                st.brush.yaw = (st.brush.yaw + by) % 4;
            } else {
                let selection = st.editor.selection.clone();
                run_command(
                    st,
                    ctx,
                    "rotate",
                    EditCommand::Rotate {
                        selection,
                        by: Yaw4::new(by % 4),
                    },
                )?;
            }
        }
        Action::Mirror {} => {
            if st.tool == Tool::Place {
                let chiral = st.kit.get(&pid(st.brush.piece)).is_some_and(|d| d.chiral);
                if !chiral {
                    return Err("the brush piece is not chiral".into());
                }
                st.brush.mirror = !st.brush.mirror;
            } else {
                let selection = st.editor.selection.clone();
                run_command(st, ctx, "mirror", EditCommand::Mirror(selection))?;
            }
        }
        Action::Remove {} => {
            let selection = st.editor.selection.clone();
            run_command(st, ctx, "remove", EditCommand::Remove(selection))?;
        }
        Action::ClearSelection {} => st.editor.selection.clear(),
        Action::Undo {} => {
            let start = Instant::now();
            if !st.editor.undo() {
                return Err("nothing to undo".into());
            }
            finish_edit(st, ctx, "undo", start);
        }
        Action::Redo {} => {
            let start = Instant::now();
            if !st.editor.redo() {
                return Err("nothing to redo".into());
            }
            finish_edit(st, ctx, "redo", start);
        }
        Action::Slice { level } => {
            st.slice = level;
            ctx.edits.slice = level;
        }
        Action::Camera {
            target,
            yaw,
            pitch,
            radius,
        } => {
            ctx.editor_cam.set = Some(Orbit {
                target: Vec3::from_array(target),
                yaw,
                pitch,
                radius,
            });
        }
        Action::Project { cells } => {
            out.projected = Some(
                cells
                    .into_iter()
                    .map(|c| {
                        let f = cell_floor_m(arr_cell(c));
                        let centre = ctx.project(f + Vec3::Y * (CELL_M * 0.5))?;
                        let floor = ctx.project(f)?;
                        Some([centre.x, centre.y, floor.x, floor.y])
                    })
                    .collect(),
            );
        }
        Action::Fill { count } => {
            if count == 0 || count > MAX_FILL {
                return Err(format!("fill: expected 1..={MAX_FILL} pieces"));
            }
            run_command(
                st,
                ctx,
                "fill",
                EditCommand::PasteGroup {
                    placements: fill_layout(count),
                    offset: CellCoord::ORIGIN,
                },
            )?;
        }
        Action::Clear {} => {
            let all: Selection = (0..st.editor.track().placements.len() as u32).collect();
            run_command(st, ctx, "clear", EditCommand::Remove(all))?;
        }
        Action::Keys { enabled } => st.keys = enabled,
        Action::Marbles { count } => {
            let hopper = marbles_hopper(st.editor.track(), count)?;
            run_command(st, ctx, "marbles", EditCommand::SetHopper(hopper))?;
        }
        Action::Run {
            release,
            interval_ticks,
        } => {
            let release = release_mode(&release, interval_ticks)?;
            start_run(st, ctx, Some(release))?;
            st.run.set_release(release);
            ctx.playback.paused = false;
            ctx.playback.step = false;
        }
        Action::Stop {} => start_run(st, ctx, None)?,
        Action::Reset {} => {
            let release = st.run.active.then_some(st.run.release);
            start_run(st, ctx, release)?;
        }
        Action::Pause { paused } => match paused {
            Some(p) => {
                ctx.playback.paused = p;
                ctx.playback.step = false;
            }
            None => ctx.playback.toggle_pause(),
        },
        Action::Follow { on } => {
            *ctx.mode = if on {
                CameraMode::Director
            } else {
                CameraMode::Orbit
            };
        }
        Action::Rename { name } => st.name = track_name(&name)?,
        Action::Export {} => {
            let json = export_json(st.editor.track(), &st.name)?;
            bridge::push_event(Event::TrackExported {
                name: st.name.clone(),
                json,
            });
        }
    }
    out.ok = true;
    Ok(out)
}

fn click(st: &mut EditState, ctx: &mut Ctx, px: Vec2, shift: bool) -> Result<(), String> {
    let hit = ctx
        .ray(px)
        .and_then(|r| st.pick_ray(r))
        .ok_or("click: the ray hits nothing")?;
    match st.tool {
        Tool::Place => {
            let target = hit.target().ok_or("click: no cell in front of the hit")?;
            let p = st.placement_at(target);
            run_command(st, ctx, "place", EditCommand::Place(p))
        }
        Tool::Select => {
            match hit {
                Hit::Piece { anchor, .. } => {
                    let i = st.index_of[&anchor];
                    if shift {
                        st.editor.selection.toggle(i);
                    } else {
                        st.editor.selection = [i].into_iter().collect();
                    }
                }
                Hit::Ground { .. } if !shift => st.editor.selection.clear(),
                Hit::Ground { .. } => {}
            }
            Ok(())
        }
        Tool::Delete => match hit {
            Hit::Piece { anchor, .. } => {
                let sel = [st.index_of[&anchor]].into_iter().collect();
                run_command(st, ctx, "delete", EditCommand::Remove(sel))
            }
            Hit::Ground { .. } => Err("click: no piece to delete".into()),
        },
    }
}

/// Editor plugin; inert unless `active`.
pub struct EditorPlugin {
    pub active: bool,
    pub fill: Option<u32>,
}

impl Plugin for EditorPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<EditorDiag>();
        if !self.active {
            return;
        }
        let mut queue: VecDeque<(Option<u32>, Action)> = VecDeque::new();
        if let Some(n) = self.fill.filter(|&n| n > 0) {
            queue.push_back((None, Action::Fill { count: n }));
        }
        inbox().extend(queue);
        app.insert_resource(EditState::new())
            .insert_resource(CameraMode::Orbit)
            .insert_resource(EditorCamera {
                active: true,
                set: Some(Orbit {
                    target: Vec3::new(1.2, 0.0, -1.2),
                    yaw: -0.5,
                    pitch: 0.95,
                    radius: 2.6,
                }),
            })
            .add_systems(
                Update,
                (
                    mouse_and_keys,
                    apply_actions,
                    update_hover,
                    rebuild_colliders,
                )
                    .chain(),
            );
    }
}

fn mouse_and_keys(
    mut st: ResMut<EditState>,
    mode: Res<CameraMode>,
    buttons: Option<Res<ButtonInput<MouseButton>>>,
    keys: Option<Res<ButtonInput<KeyCode>>>,
    windows: Query<&Window, With<PrimaryWindow>>,
) {
    let cursor = windows.single().ok().and_then(Window::cursor_position);
    if cursor.is_some() && cursor != st.last_cursor {
        if st.last_cursor.is_some() {
            st.hover_src = HoverSrc::Cursor;
        }
        st.last_cursor = cursor;
    }
    // Editing works under the orbit and follow cameras; the fly camera owns the mouse and keys.
    if *mode == CameraMode::Fly {
        return;
    }
    let mut out: Vec<Action> = Vec::new();
    let shift = keys
        .as_ref()
        .is_some_and(|k| k.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]));
    if let (Some(b), Some(c)) = (buttons.as_ref(), cursor) {
        if b.just_pressed(MouseButton::Left) {
            st.press = Some((c, Press::Box));
            st.dragging = false;
        }
        if let Some((p0, _)) = st.press
            && b.pressed(MouseButton::Left)
            && !st.dragging
            && c.distance(p0) > DRAG_PX
        {
            st.dragging = true;
        }
        if b.just_released(MouseButton::Left)
            && let Some((p0, kind)) = st.press.take()
        {
            let (x0, y0, x1, y1) = (p0.x, p0.y, c.x, c.y);
            if !st.dragging {
                out.push(Action::Click {
                    x: x1,
                    y: y1,
                    shift,
                });
            } else if st.tool == Tool::Select {
                match kind {
                    Press::Move { .. } => out.push(Action::Drag { x0, y0, x1, y1 }),
                    Press::Box => out.push(Action::BoxSelectScreen {
                        x0,
                        y0,
                        x1,
                        y1,
                        shift,
                    }),
                }
            }
            st.dragging = false;
        }
    }
    if let Some(k) = keys {
        out.extend(key_actions(&st, &k, shift));
    }
    if !out.is_empty() {
        inbox().extend(out.into_iter().map(|a| (None, a)));
    }
}

/// The editor shortcuts pressed this frame; none while `keys` is off.
fn key_actions(st: &EditState, k: &ButtonInput<KeyCode>, shift: bool) -> Vec<Action> {
    let mut out = Vec::new();
    if !st.keys {
        return out;
    }
    let ctrl = k.any_pressed([
        KeyCode::ControlLeft,
        KeyCode::ControlRight,
        KeyCode::SuperLeft,
        KeyCode::SuperRight,
    ]);
    let max_level = st.by_anchor.keys().map(|a| a.y).max().unwrap_or(0);
    for key in k.get_just_pressed() {
        match key {
            KeyCode::Digit1 => out.push(Action::Tool { tool: Tool::Place }),
            KeyCode::Digit2 => out.push(Action::Tool { tool: Tool::Select }),
            KeyCode::Digit3 => out.push(Action::Tool { tool: Tool::Delete }),
            KeyCode::KeyR => out.push(Action::Rotate { by: 1 }),
            KeyCode::KeyM => out.push(Action::Mirror {}),
            KeyCode::Delete | KeyCode::Backspace => out.push(Action::Remove {}),
            KeyCode::KeyZ if ctrl && shift => out.push(Action::Redo {}),
            KeyCode::KeyZ if ctrl => out.push(Action::Undo {}),
            KeyCode::KeyY if ctrl => out.push(Action::Redo {}),
            KeyCode::BracketLeft => out.push(Action::Slice {
                level: Some((st.slice.unwrap_or(max_level + 1) - 1).max(0)),
            }),
            KeyCode::BracketRight => out.push(Action::Slice {
                level: st.slice.map(|s| s + 1).filter(|&s| s <= max_level),
            }),
            KeyCode::KeyX => out.push(Action::Slice { level: None }),
            KeyCode::Escape => out.push(Action::ClearSelection {}),
            KeyCode::Tab => {
                let next = (st.brush.piece + 1) % kit::PIECES.len();
                out.push(Action::Brush {
                    piece: kit::PIECES[next].0.to_owned(),
                    yaw: st.brush.yaw,
                    mirror: false,
                });
            }
            _ => {}
        }
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn apply_actions(
    mut st: ResMut<EditState>,
    mut clock: ResMut<SimClock>,
    mut edits: ResMut<KitEdits>,
    mut diag: ResMut<EditorDiag>,
    mut editor_cam: ResMut<EditorCamera>,
    mut frame: ResMut<MarbleFrame>,
    mut parts: ResMut<PartFrame>,
    mut playback: ResMut<Playback>,
    mut mode: ResMut<CameraMode>,
    cams: Query<(&Camera, &GlobalTransform), With<MainCamera>>,
) {
    if st.restarts != clock.restarts {
        st.restarts = clock.restarts;
        let file = clock.tracks.loaded.as_ref();
        let track = file.map_or_else(|| Track::new(0), |f| f.track.clone());
        st.name = file
            .map(|f| f.metadata.name.trim())
            .filter(|n| !n.is_empty())
            .unwrap_or(UNTITLED)
            .to_owned();
        // The restarted sim runs the loaded file's own hopper config.
        st.run.active = track.hopper.placement.is_some() && !track.hopper.queue.is_empty();
        st.run.set_release(track.hopper.release);
        st.adopt(track, &mut edits);
        // The ghost and selection marks are keyed on the edit count.
        diag.edits += 1;
    }
    let mut actions: Vec<_> = inbox().drain(..).collect();
    // The frame's mouse and key actions (no id) first, so `last` ends on the agent's action.
    actions.sort_by_key(|(id, _)| id.is_some());
    let camera = cams.single().ok();
    let st = &mut *st;
    let mut ctx = Ctx {
        clock: &mut clock,
        edits: &mut edits,
        diag: &mut diag,
        camera,
        editor_cam: &mut editor_cam,
        frame: &mut frame,
        parts: &mut parts,
        playback: &mut playback,
        mode: &mut mode,
    };
    for (id, action) in actions {
        let op = op_name(&action);
        let mut last = apply_action(st, &mut ctx, action).unwrap_or_else(|e| LastAction {
            error: Some(e),
            ..default()
        });
        last.id = id;
        last.op = op;
        ctx.diag.seq += 1;
        ctx.diag.last = last;
    }
    // Press kind for a drag that starts on a selected piece (needs the camera and the pick).
    if let Some((p0, Press::Box)) = st.press
        && st.tool == Tool::Select
        && !st.dragging
        && let Some(Hit::Piece { anchor, .. }) = ctx.ray(p0).and_then(|r| st.pick_ray(r))
        && st
            .index_of
            .get(&anchor)
            .is_some_and(|&i| st.editor.selection.contains(i))
    {
        st.press = Some((p0, Press::Move { anchor }));
    }
    let d = &mut *ctx.diag;
    d.tool = st.tool;
    d.brush = BrushDiag {
        piece: kit::PIECES[st.brush.piece].0,
        yaw: st.brush.yaw,
        mirror: st.brush.mirror,
    };
    d.palette = palette();
    d.pieces = st.by_anchor.len();
    d.slice = st.slice;
    d.visible_pieces = st.by_anchor.keys().filter(|a| st.visible(**a)).count();
    d.selection = st.editor.selection.len();
    let placements = &st.editor.track().placements;
    d.selected = st
        .editor
        .selection
        .iter()
        .take(MAX_LISTED)
        .filter_map(|i| placements.get(i as usize).map(|p| cell_arr(p.anchor)))
        .collect();
    d.can_undo = st.editor.can_undo();
    d.can_redo = st.editor.can_redo();
    d.keys = st.keys;
    d.name.clone_from(&st.name);
    d.run = RunDiag {
        active: st.run.active,
        release: release_name(st.run.release),
        interval_ticks: st.run.interval,
        queue: queue_len(&st.editor.track().hopper),
        marbles: ctx.frame.curr.len(),
        tick: ctx.clock.sim.tick_count(),
        paused: ctx.playback.paused,
        follow: *ctx.mode == CameraMode::Director,
    };
    // Validation and the selection's make-up only when the track or the selection changes.
    if st.checked_at != Some(d.edits) {
        st.checked_at = Some(d.edits);
        let track = resolve_hopper(st.editor.track());
        (d.issue_count, d.issues) = issue_diags(&track, st.kit);
        d.hoppers = track
            .placements
            .iter()
            .filter(|p| is_hopper(&p.piece))
            .count();
        d.finishes = track
            .placements
            .iter()
            .filter(|p| is_finish(&p.piece))
            .count();
    }
    if st
        .selection_at
        .as_ref()
        .is_none_or(|(e, s)| *e != d.edits || *s != st.editor.selection)
    {
        st.selection_at = Some((d.edits, st.editor.selection.clone()));
        d.selection_info = selection_diag(st);
    }
}

#[allow(clippy::too_many_arguments)]
fn update_hover(
    mut st: ResMut<EditState>,
    mut diag: ResMut<EditorDiag>,
    mut ghost: ResMut<KitGhost>,
    mut marks: ResMut<KitMarks>,
    windows: Query<&Window, With<PrimaryWindow>>,
    cams: Query<(&Camera, &GlobalTransform), With<MainCamera>>,
) {
    let camera = cams.single().ok();
    let ray = |px: Vec2| camera.and_then(|(c, gt)| c.viewport_to_world(gt, px).ok());
    let cursor = windows.single().ok().and_then(Window::cursor_position);
    let (source, hit) = match st.hover_src {
        HoverSrc::Cursor => ("cursor", cursor.and_then(ray).and_then(|r| st.pick_ray(r))),
        HoverSrc::Pixel(px) => ("pixel", ray(px).and_then(|r| st.pick_ray(r))),
        HoverSrc::Cell(_) => ("cell", None),
    };
    let target = match st.hover_src {
        HoverSrc::Cell(c) => Some(c),
        _ if st.tool == Tool::Place => hit.and_then(Hit::target),
        _ => None,
    };
    let edits = diag.edits;
    let key = (target, st.brush.piece, st.brush.yaw, st.brush.mirror, edits);
    let hover_known = st.hover_src == HoverSrc::Cursor && cursor.is_none();
    if st.ghost_key != Some(key) {
        st.ghost_key = Some(key);
        let (valid, error, ports, piece) = match target {
            Some(t) => {
                let p = st.placement_at(t);
                let check = st.editor.check(st.kit, EditCommand::Place(p.clone()));
                (
                    check.is_ok(),
                    check.err().map(|e| e.to_string()),
                    st.port_hints(&p),
                    edit_piece(&p),
                )
            }
            None => (false, None, Vec::new(), None),
        };
        ghost.piece = piece;
        ghost.valid = valid;
        ghost.ports = ports
            .iter()
            .map(|pd| {
                let f = pd.face.normal();
                let base = cell_floor_m(arr_cell(pd.cell));
                let y = if pd.face.is_side() {
                    CELL_M * 0.55
                } else if f.y > 0 {
                    CELL_M
                } else {
                    0.0
                };
                let pos =
                    base + Vec3::new(f.x as f32, 0.0, f.z as f32) * (CELL_M * 0.5) + Vec3::Y * y;
                (pos, pd.hint)
            })
            .collect();
        diag.hover = (!hover_known).then(|| HoverDiag {
            source,
            hit: None,
            hit_cell: None,
            hit_anchor: None,
            target: target.map(cell_arr),
            valid,
            error,
            ports,
        });
    }
    if let Some(h) = diag.hover.as_mut() {
        h.source = source;
        h.hit = hit.map(|h| match h {
            Hit::Piece { .. } => "piece",
            Hit::Ground { .. } => "ground",
        });
        h.hit_cell = hit.map(|h| match h {
            Hit::Piece { cell, .. } | Hit::Ground { cell } => cell_arr(cell),
        });
        h.hit_anchor = hit.and_then(|h| match h {
            Hit::Piece { anchor, .. } => Some(cell_arr(anchor)),
            Hit::Ground { .. } => None,
        });
    }
    if hover_known {
        diag.hover = None;
        if ghost.piece.is_some() {
            ghost.piece = None;
        }
    }

    // Selection marks, and the live move-drag preview.
    let mut offset = CellCoord::ORIGIN;
    let mut drag = None;
    if let (Some((p0, Press::Move { anchor })), true, Some(c)) = (st.press, st.dragging, cursor) {
        let ctx_ray = |px: Vec2| ray(px);
        if let (Some(r0), Some(r1)) = (ctx_ray(p0), ctx_ray(c))
            && let (Some(a), Some(b)) = (
                plane_cell(r0.origin, *r0.direction, anchor.y),
                plane_cell(r1.origin, *r1.direction, anchor.y),
            )
        {
            offset = CellCoord::new(b.x - a.x, 0, b.z - a.z);
            let selection = drag_selection(&st, anchor);
            let valid = offset == CellCoord::ORIGIN
                || st
                    .editor
                    .check(st.kit, EditCommand::MoveGroup { selection, offset })
                    .is_ok();
            drag = Some(DragDiag {
                offset: cell_arr(offset),
                valid,
            });
        }
    }
    let mkey = (edits, st.editor.selection.len());
    if st.marks_key != Some(mkey) || drag.is_some() || diag.drag.is_some() {
        st.marks_key = Some(mkey);
        let placements = &st.editor.track().placements;
        marks.cells = st
            .editor
            .selection
            .iter()
            .take(MAX_MARKS)
            .filter_map(|i| placements.get(i as usize).map(|p| ivec(p.anchor)))
            .collect();
        marks.offset = ivec(offset);
        marks.valid = drag.as_ref().is_none_or(|d| d.valid);
    }
    diag.drag = drag;
}

/// Rebuild dirty chunk colliders inside [`COLLIDER_BUDGET_MS`] per frame.
fn rebuild_colliders(
    mut st: ResMut<EditState>,
    mut clock: ResMut<SimClock>,
    mut diag: ResMut<EditorDiag>,
) {
    let kit = st.kit;
    let frame = Instant::now();
    let mut chunks = 0;
    let mut steps = 0;
    loop {
        if frame.elapsed().as_secs_f32() * 1e3 >= COLLIDER_BUDGET_MS {
            break;
        }
        let one = Instant::now();
        match clock.sim.rebuild_step_chunk(kit) {
            Ok(Some((_, done))) => {
                steps += 1;
                chunks += usize::from(done);
                let ms = one.elapsed().as_secs_f32() * 1e3;
                if !st.pending_is_fill {
                    diag.stats.collider_step_ms_max = diag.stats.collider_step_ms_max.max(ms);
                }
            }
            Ok(None) => break,
            Err(e) => {
                error!("editor: collider rebuild: {e}");
                break;
            }
        }
    }
    let ms = frame.elapsed().as_secs_f32() * 1e3;
    let dirty = clock.sim.chunk_colliders().dirty_chunks().count();
    if steps > 0 {
        let fill = st.pending_is_fill;
        if let Some(t) = st.pending.as_mut() {
            t.collider_ms += ms;
            t.collider_chunks += chunks;
            t.collider_frames += 1;
            diag.last_edit = Some(t.clone());
        }
        if !fill {
            diag.stats.collider_frame_ms_max = diag.stats.collider_frame_ms_max.max(ms);
            diag.stats.collider_ms_total += ms;
            diag.stats.collider_chunks_total += chunks;
        }
    }
    if dirty == 0
        && let Some(t) = st.pending.take()
    {
        if st.pending_is_fill {
            diag.stats.fill = Some(t);
        }
        st.pending_is_fill = false;
    }
    diag.dirty_collider_chunks = dirty;
    diag.collider_chunks = clock.sim.chunk_colliders().chunks().count();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(x: i32, y: i32, z: i32) -> CellCoord {
        CellCoord::new(x, y, z)
    }

    /// World position of the centre of a cell.
    fn centre(cell: CellCoord) -> Vec3 {
        cell_floor_m(cell) + Vec3::Y * (CELL_M * 0.5)
    }

    #[test]
    fn a_ray_from_above_lands_on_the_ground_cell_below() {
        let eye = Vec3::new(0.3, 1.0, -0.12);
        let hit = pick(eye, Vec3::NEG_Y, 0, |_| None).unwrap();
        assert_eq!(hit, Hit::Ground { cell: c(5, 0, -2) });
        assert_eq!(hit.target(), Some(c(5, 0, -2)));
        // A slanted ray lands where it crosses y = 0.
        let to = cell_floor_m(c(-3, 0, 7)) + Vec3::new(0.01, 0.0, -0.01);
        let hit = pick(
            Vec3::new(1.0, 0.8, 0.2),
            to - Vec3::new(1.0, 0.8, 0.2),
            0,
            |_| None,
        );
        assert_eq!(hit, Some(Hit::Ground { cell: c(-3, 0, 7) }));
    }

    #[test]
    fn a_ray_stops_at_the_first_piece_and_reports_its_face() {
        let occ = |cell: CellCoord| (cell == c(2, 0, 2) || cell == c(2, 1, 2)).then_some(cell);
        let down = pick(centre(c(2, 5, 2)), Vec3::NEG_Y, 0, occ).unwrap();
        assert_eq!(
            down,
            Hit::Piece {
                cell: c(2, 1, 2),
                anchor: c(2, 1, 2),
                normal: c(0, 1, 0)
            }
        );
        assert_eq!(down.target(), Some(c(2, 2, 2)));
        let side = pick(centre(c(-4, 0, 2)), Vec3::X, 0, occ).unwrap();
        assert_eq!(side.target(), Some(c(1, 0, 2)));
    }

    #[test]
    fn slicing_moves_the_ground_plane_and_skips_hidden_pieces() {
        // The occupant filter is where slicing hides pieces; the plane follows the slice level.
        let hit = pick(centre(c(0, 6, 0)), Vec3::NEG_Y, 2, |_| None).unwrap();
        assert_eq!(hit, Hit::Ground { cell: c(0, 2, 0) });
        assert_eq!(
            plane_cell(centre(c(1, 6, 1)), Vec3::NEG_Y, 3),
            Some(c(1, 3, 1))
        );
        assert_eq!(plane_cell(centre(c(1, 6, 1)), Vec3::Y, 3), None);
    }

    #[test]
    fn fill_is_valid_and_lands_in_one_paste() {
        let mut st = EditState::new();
        let placements = fill_layout(2_000);
        st.editor
            .apply(
                st.kit,
                EditCommand::PasteGroup {
                    placements,
                    offset: CellCoord::ORIGIN,
                },
            )
            .unwrap();
        assert_eq!(st.editor.track().placements.len(), 2_000);
        let chunks: BTreeSet<_> = st
            .editor
            .track()
            .placements
            .iter()
            .map(|p| chunk_of_cell(ivec(p.anchor)))
            .collect();
        assert!(chunks.len() >= 35, "{} chunks", chunks.len());
    }

    #[test]
    fn a_loaded_track_replaces_the_pieces_selection_and_history() {
        let mut st = EditState::new();
        let mut edits = KitEdits::default();
        // Before the load: an edited track with history and a selection, its chunks drawn.
        st.adopt(Track::new(0), &mut edits);
        st.editor
            .apply(
                st.kit,
                EditCommand::PasteGroup {
                    placements: fill_layout(120),
                    offset: CellCoord::ORIGIN,
                },
            )
            .unwrap();
        let before: Track = st.editor.track().clone();
        st.adopt(before, &mut edits);
        st.editor
            .apply(st.kit, EditCommand::Place(st.placement_at(c(0, 3, 0))))
            .unwrap();
        st.editor.selection = st.selection_for([c(0, 0, -5), c(1, 0, -5)]);
        assert!(st.editor.can_undo());
        assert_eq!(st.editor.selection.len(), 2);
        let old_chunks: BTreeSet<[i32; 3]> = st.by_chunk.keys().copied().collect();
        edits.pending.clear();

        let mut loaded = Track::new(0);
        loaded.placements = vec![
            Placement::new(pid(0), c(40, 0, 10), Yaw4::new(0), Mirror::Off),
            Placement::new(pid(1), c(41, 0, 10), Yaw4::new(1), Mirror::On),
        ];
        st.adopt(loaded.clone(), &mut edits);

        assert_eq!(st.editor.track(), &loaded);
        assert!(!st.editor.can_undo() && !st.editor.can_redo());
        assert!(st.editor.selection.is_empty());
        assert_eq!(
            st.by_anchor.keys().copied().collect::<Vec<_>>(),
            [c(40, 0, 10), c(41, 0, 10)]
        );
        assert_eq!(st.occupant(c(41, 0, 10)), Some(c(41, 0, 10)));
        assert_eq!(st.occupant(c(0, 0, -5)), None);
        assert_eq!(st.index_of[&c(41, 0, 10)], 1);
        // Every old chunk is cleared and the loaded track's chunk drawn, mirror included.
        let new_chunk = chunk_of_cell(IVec3::new(40, 0, 10));
        for ch in &old_chunks {
            assert_eq!(edits.pending.get(ch), Some(&Vec::new()), "chunk {ch:?}");
        }
        assert_eq!(
            edits.pending[&new_chunk],
            loaded
                .placements
                .iter()
                .filter_map(edit_piece)
                .collect::<Vec<_>>()
        );
        assert!(edits.pending[&new_chunk][1].mirror);
        assert_eq!(edits.pending.len(), old_chunks.len() + 1);
    }

    #[test]
    fn port_hints_link_two_straights_and_flag_a_wrong_turn() {
        let mut st = EditState::new();
        let straight = pid(piece_index("blk_straight_l3").unwrap());
        st.editor
            .apply(
                st.kit,
                EditCommand::Place(Placement::new(
                    straight.clone(),
                    c(0, 0, 0),
                    Yaw4::new(0),
                    Mirror::Off,
                )),
            )
            .unwrap();
        let p = st.editor.track().placements[0].clone();
        st.by_anchor.insert(p.anchor, p.clone());
        st.occupancy.insert(p.anchor, p.anchor);
        let def = st.kit.get(&straight).unwrap().clone();
        // The cell across the straight's first port, with the same yaw: its ports face back.
        let port = def.ports[0];
        let next = port.cell + port.face.normal();
        let hints = st.port_hints(&Placement::new(
            straight.clone(),
            next,
            Yaw4::new(0),
            Mirror::Off,
        ));
        assert!(hints.contains(&PortDiag {
            cell: cell_arr(next),
            face: port.face.opposite(),
            hint: PortHint::Linked
        }));
        assert!(hints.iter().any(|h| h.hint == PortHint::Open));
        // Turned a quarter, its port no longer faces the straight: a fault across.
        let turned = st.port_hints(&Placement::new(straight, next, Yaw4::new(1), Mirror::Off));
        assert!(!turned.iter().any(|h| h.hint == PortHint::Linked));
    }

    #[test]
    fn actions_parse_with_and_without_ids() {
        post_action(r#"{"op":"place","piece":"blk_end_l3","cell":[1,0,2],"id":4}"#).unwrap();
        post_action(r#"{"op":"slice","level":null}"#).unwrap();
        post_action(r#"{"op":"undo"}"#).unwrap();
        assert!(post_action(r#"{"op":"teleport"}"#).is_err());
        let got: Vec<_> = inbox().drain(..).collect();
        assert_eq!(got.len(), 3);
        assert_eq!(got[0].0, Some(4));
        assert_eq!(got[1].1, Action::Slice { level: None });
    }

    fn placed(id: &str, cell: CellCoord) -> Placement {
        Placement::new(
            pid(piece_index(id).unwrap()),
            cell,
            Yaw4::new(0),
            Mirror::Off,
        )
    }

    fn parse(json: &str) -> Action {
        serde_json::from_str::<Envelope>(json).unwrap().action
    }

    #[test]
    fn keys_off_drops_every_shortcut() {
        let mut st = EditState::new();
        let mut k = ButtonInput::<KeyCode>::default();
        k.press(KeyCode::Digit2);
        assert_eq!(
            key_actions(&st, &k, false),
            [Action::Tool { tool: Tool::Select }]
        );
        st.keys = false;
        assert!(key_actions(&st, &k, false).is_empty());
        assert_eq!(
            parse(r#"{"op":"keys","enabled":false}"#),
            Action::Keys { enabled: false }
        );
    }

    #[test]
    fn the_hopper_resolves_to_the_first_hopper_placed() {
        let mut track = Track::new(0);
        track.placements = vec![
            placed("blk_straight_l3", c(0, 0, 0)),
            placed("mech_hopper", c(5, 0, 5)),
            placed("mech_hopper", c(10, 0, 10)),
        ];
        // Placing a hopper never sets the config: the first hopper is the source.
        let t = resolve_hopper(&track);
        assert_eq!(t.hopper.placement, Some(1));
        assert_eq!(t.hopper.queue, vec![MarbleSize::Standard; 10]);
        // A stored hopper index is kept; a stale one (not a hopper) is replaced.
        track.hopper.placement = Some(2);
        track.hopper.queue = vec![MarbleSize::Standard; 3];
        let t = resolve_hopper(&track);
        assert_eq!((t.hopper.placement, t.hopper.queue.len()), (Some(2), 3));
        track.hopper.placement = Some(0);
        assert_eq!(resolve_hopper(&track).hopper.placement, Some(1));
        track.placements.truncate(1);
        assert_eq!(resolve_hopper(&track).hopper.placement, None);
        assert_eq!(queue_len(&HopperConfig::default()), 10);
    }

    #[test]
    fn a_run_releases_the_resolved_hopper_and_stop_idles() {
        let mut track = Track::new(0);
        track.placements = vec![placed("mech_hopper", c(0, 0, 0))];
        let mut run = editor_run(&track, Some(ReleaseMode::All)).unwrap();
        run.tick(480);
        assert_eq!(run.marbles().len(), 10);
        let mut idle = editor_run(&track, None).unwrap();
        idle.tick(480);
        assert!(idle.marbles().is_empty());
        assert_eq!(
            editor_run(&Track::new(0), Some(ReleaseMode::One)).err(),
            Some("no hopper".to_owned())
        );
        assert_eq!(release_mode("one", None), Ok(ReleaseMode::One));
        assert_eq!(
            release_mode("interval", None),
            Ok(ReleaseMode::Interval { ticks: 240 })
        );
        assert_eq!(
            release_mode("interval", Some(60)),
            Ok(ReleaseMode::Interval { ticks: 60 })
        );
        assert!(release_mode("interval", Some(0)).is_err());
        assert!(release_mode("burst", None).is_err());
        for mode in ["one", "all", "interval", "sequential"] {
            assert_eq!(release_name(release_mode(mode, None).unwrap()), mode);
        }
        let mut rs = EditState::new().run;
        rs.set_release(ReleaseMode::Interval { ticks: 90 });
        rs.set_release(ReleaseMode::All);
        assert_eq!((rs.release, rs.interval), (ReleaseMode::All, 90));
        assert_eq!(
            parse(r#"{"op":"run","release":"interval","intervalTicks":120}"#),
            Action::Run {
                release: "interval".into(),
                interval_ticks: Some(120)
            }
        );
        assert_eq!(parse(r#"{"op":"pause"}"#), Action::Pause { paused: None });
        assert_eq!(
            parse(r#"{"op":"follow","on":true}"#),
            Action::Follow { on: true }
        );
        assert_eq!(parse(r#"{"op":"reset"}"#), Action::Reset {});
        assert_eq!(parse(r#"{"op":"stop"}"#), Action::Stop {});
    }

    #[test]
    fn marbles_sets_an_undoable_queue_on_the_resolved_hopper() {
        let mut st = EditState::new();
        st.editor
            .apply(
                st.kit,
                EditCommand::Place(placed("blk_straight_l3", c(0, 0, 0))),
            )
            .unwrap();
        st.editor
            .apply(
                st.kit,
                EditCommand::Place(placed("mech_hopper", c(5, 0, 5))),
            )
            .unwrap();
        assert!(marbles_hopper(st.editor.track(), 0).is_err());
        assert!(marbles_hopper(st.editor.track(), 11).is_err());
        let h = marbles_hopper(st.editor.track(), 4).unwrap();
        st.editor.apply(st.kit, EditCommand::SetHopper(h)).unwrap();
        let hopper = &st.editor.track().hopper;
        assert_eq!(hopper.placement, Some(1));
        assert_eq!(hopper.queue, vec![MarbleSize::Standard; 4]);
        assert_eq!(hopper.release, ReleaseMode::default());
        assert_eq!(queue_len(hopper), 4);
        assert!(st.editor.undo());
        assert_eq!(st.editor.track().hopper, HopperConfig::default());
        assert_eq!(queue_len(&st.editor.track().hopper), 10);
    }

    #[test]
    fn issues_name_their_kind_piece_face_and_fault() {
        let st = EditState::new();
        let straight = placed("blk_straight_l3", c(0, 0, 0));
        let def = st.kit.get(&straight.piece).unwrap().clone();
        let port = def.ports[0];
        // A straight across the first port, turned a quarter: a broken link between the two.
        let mut turned = placed("blk_straight_l3", port.cell + port.face.normal());
        turned.yaw = Yaw4::new(1);
        let mut track = Track::new(0);
        track.placements = vec![straight, turned];
        let (count, issues) = issue_diags(&resolve_hopper(&track), st.kit);
        assert_eq!(count, issues.len());
        let kinds: BTreeSet<&str> = issues.iter().map(|i| i.kind).collect();
        assert!(kinds.contains("missingHopper"), "{kinds:?}");
        assert!(kinds.contains("missingFinish"), "{kinds:?}");
        let broken = issues.iter().find(|i| i.kind == "brokenLink").unwrap();
        assert_eq!(broken.piece.as_deref(), Some("blk_straight_l3"));
        assert_eq!(broken.other_piece.as_deref(), Some("blk_straight_l3"));
        assert_eq!(broken.anchor, Some([0, 0, 0]));
        assert!(broken.cell.is_some() && broken.fault.is_some());
        assert_eq!(broken.face, Some(port.face));
        let missing = issues.iter().find(|i| i.kind == "missingHopper").unwrap();
        assert_eq!((missing.piece.as_ref(), missing.face), (None, None));
        // Open ports carry their world face; the list stops at 32, the count does not.
        let (count, issues) = issue_diags(
            &Track {
                placements: fill_layout(200),
                ..Track::new(0)
            },
            st.kit,
        );
        assert_eq!(issues.len(), MAX_ISSUES);
        assert!(count > MAX_ISSUES);
        assert!(
            issues
                .iter()
                .any(|i| i.kind == "openPort" && i.face.is_some())
        );
    }

    /// The validator on a 2,000-piece fill with a hopper as its source (every piece then gets the
    /// reachability pass). A timing note, not a gate: run with `--nocapture` to see it.
    #[test]
    fn issues_on_a_2000_piece_fill_time() {
        let st = EditState::new();
        let mut track = Track::new(0);
        track.placements = fill_layout(2_000);
        track.placements.push(placed("mech_hopper", c(-10, 0, 10)));
        let track = resolve_hopper(&track);
        let start = Instant::now();
        let (count, _) = issue_diags(&track, st.kit);
        let ms = start.elapsed().as_secs_f32() * 1e3;
        println!("validate_track: 2,001 pieces, {count} issues, {ms:.1} ms");
        assert!(count > 0);
        // `run`, `stop` and `reset` build the whole sim of the same track.
        let start = Instant::now();
        let run = editor_run(&track, Some(ReleaseMode::All)).unwrap();
        let ms = start.elapsed().as_secs_f32() * 1e3;
        println!(
            "editor_run: 2,001 pieces, {} chunks, {ms:.0} ms",
            run.chunk_colliders().chunks().count()
        );
    }

    #[test]
    fn selection_info_counts_families_levels_and_one_piece_ports() {
        let mut st = EditState::new();
        let mut track = Track::new(0);
        track.placements = vec![
            placed("blk_straight_l3", c(0, 0, 0)),
            placed("blk_pillar", c(3, 0, 0)),
            placed("rail_straight_l1", c(6, 2, 0)),
            placed("mech_finish", c(9, 1, 0)),
        ];
        st.adopt(track, &mut KitEdits::default());
        assert_eq!(selection_diag(&st), None);
        st.editor.selection = (0..4).collect();
        let info = selection_diag(&st).unwrap();
        assert_eq!(info.count, 4);
        assert_eq!(
            info.families,
            [("blk", 1), ("mech", 1), ("rail", 1), ("support", 1)]
                .map(|(k, v)| (k.to_owned(), v))
                .into()
        );
        assert_eq!(info.levels, [0, 2]);
        assert!(info.single.is_none());
        st.editor.selection = [0].into_iter().collect();
        let one = selection_diag(&st).unwrap().single.unwrap();
        assert_eq!(
            (one.piece.as_str(), one.anchor, one.yaw, one.mirror),
            ("blk_straight_l3", [0, 0, 0], 0, false)
        );
        let def = st
            .kit
            .get(&pid(piece_index("blk_straight_l3").unwrap()))
            .unwrap();
        assert_eq!(one.ports.len(), def.ports.len());
        for (p, q) in one.ports.iter().zip(&def.ports) {
            assert_eq!((p.face, p.flow, p.hint), (q.face, q.flow, PortHint::Open));
        }
    }

    #[test]
    fn rename_trims_and_export_writes_the_resolved_track() {
        assert_eq!(track_name("  Loop de loop "), Ok("Loop de loop".to_owned()));
        assert!(track_name("   ").is_err());
        assert!(track_name(&"x".repeat(121)).is_err());
        assert!(track_name(&"x".repeat(120)).is_ok());
        assert_eq!(
            parse(r#"{"op":"rename","name":"A"}"#),
            Action::Rename { name: "A".into() }
        );
        assert_eq!(parse(r#"{"op":"export"}"#), Action::Export {});
        assert_eq!(
            parse(r#"{"op":"marbles","count":3}"#),
            Action::Marbles { count: 3 }
        );
        let mut track = Track::new(7);
        track.placements = vec![
            placed("blk_straight_l3", c(0, 0, 0)),
            placed("mech_hopper", c(5, 0, 5)),
        ];
        let json = export_json(&track, "Loop de loop").unwrap();
        let file = parble_core::file::load(&json).unwrap();
        assert_eq!(file.metadata.name, "Loop de loop");
        assert_eq!(file.track, resolve_hopper(&track));
        assert_eq!(file.track.hopper.placement, Some(1));
    }

    fn act(st: &mut EditState, ctx: &mut Ctx, json: &str) -> Result<LastAction, String> {
        apply_action(st, ctx, parse(json))
    }

    #[test]
    fn run_ops_swap_the_sim_and_keep_the_edits_and_undo() {
        let mut st = EditState::new();
        let mut clock = SimClock {
            sim: TrackRun::from_sim(parble_sim::Sim::tray_scene()),
            scenario: crate::agent::Scenario::Editor,
            tracks: Default::default(),
            marble: None,
            marble_size: parble_sim::marble::MarbleSize::Standard,
            acc: 0.0,
            runs: 0,
            last_run_hash: None,
            restarts: 0,
        };
        let (mut edits, mut diag, mut editor_cam) = Default::default();
        let (mut frame, mut parts, mut playback) = Default::default();
        let mut mode = CameraMode::Orbit;
        let ctx = &mut Ctx {
            clock: &mut clock,
            edits: &mut edits,
            diag: &mut diag,
            camera: None,
            editor_cam: &mut editor_cam,
            frame: &mut frame,
            parts: &mut parts,
            playback: &mut playback,
            mode: &mut mode,
        };
        let st = &mut st;
        assert_eq!(
            act(st, ctx, r#"{"op":"run","release":"all"}"#).err(),
            Some("no hopper".to_owned())
        );
        act(
            st,
            ctx,
            r#"{"op":"place","piece":"mech_hopper","cell":[0,0,0]}"#,
        )
        .unwrap();
        act(
            st,
            ctx,
            r#"{"op":"place","piece":"blk_straight_l3","cell":[5,0,5]}"#,
        )
        .unwrap();
        act(st, ctx, r#"{"op":"marbles","count":4}"#).unwrap();
        act(st, ctx, r#"{"op":"pause","paused":true}"#).unwrap();
        act(st, ctx, r#"{"op":"run","release":"all"}"#).unwrap();
        // The swap keeps the edits and their history, and never looks like a bridge restart.
        assert!(st.editor.can_undo());
        assert_eq!(st.editor.track().placements.len(), 2);
        assert_eq!(ctx.clock.restarts, 0);
        assert!(st.pending.is_none() && st.run.active);
        assert!(!ctx.playback.paused);
        assert_eq!(ctx.clock.sim.tick_count(), 0);
        assert_eq!(ctx.frame.curr.len(), ctx.clock.sim.marbles().len());
        ctx.clock.sim.tick(480);
        assert_eq!(ctx.clock.sim.marbles().len(), 4);
        // An edit mid-run reaches the run's colliders.
        act(
            st,
            ctx,
            r#"{"op":"place","piece":"blk_straight_l3","cell":[8,0,8]}"#,
        )
        .unwrap();
        assert!(ctx.clock.sim.chunk_colliders().dirty_chunks().count() > 0);
        act(st, ctx, r#"{"op":"reset"}"#).unwrap();
        assert_eq!(ctx.clock.sim.tick_count(), 0);
        assert_eq!(ctx.clock.sim.chunk_colliders().dirty_chunks().count(), 0);
        assert!(st.run.active && st.editor.can_undo());
        ctx.clock.sim.tick(480);
        assert_eq!(ctx.clock.sim.marbles().len(), 4);
        act(st, ctx, r#"{"op":"stop"}"#).unwrap();
        assert!(!st.run.active);
        ctx.clock.sim.tick(480);
        assert!(ctx.clock.sim.marbles().is_empty());
        act(st, ctx, r#"{"op":"reset"}"#).unwrap();
        assert!(!st.run.active && ctx.frame.curr.is_empty());
        assert_eq!(st.editor.track().placements.len(), 3);
        act(st, ctx, r#"{"op":"follow","on":true}"#).unwrap();
        assert_eq!(*ctx.mode, CameraMode::Director);
        act(st, ctx, r#"{"op":"follow","on":false}"#).unwrap();
        assert_eq!(*ctx.mode, CameraMode::Orbit);
        act(st, ctx, r#"{"op":"pause"}"#).unwrap();
        assert!(ctx.playback.paused);
        assert_eq!(ctx.clock.restarts, 0);
    }
}
