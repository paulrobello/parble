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
//! - Run, Stop and Reset build the new sim with a [`TrackRunBuild`] spread over frames
//!   ([`RUN_BUILD_BUDGET_MS`] per frame, in place of the collider budget) while the live sim holds
//!   still, then install it; an edit meanwhile starts the build over on the edited track. On a sim
//!   built that way, an edit also adds and removes the changed placements' mechanism instances,
//!   so a mechanism moved mid-run works at its new cell (P3-T5).
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
    NudgeZone, Orientation, PieceCatalog, PieceId, Placement, PortFace, PortFlow, ReleaseMode,
    Selection, Track, Yaw4, file::ParbleFile, is_finish, is_hopper, link_fault, validate_track,
};
use parble_kit::KitCatalog;
use parble_render::{
    MarbleFrame,
    camera::{CameraMode, EditorCamera, MainCamera, Orbit},
    kit::{
        self, CELL_M,
        edit::{
            ColumnTint, EditPiece, KitColumns, KitEdits, KitGhost, KitMarks, MAX_COLUMNS,
            MAX_MARKS, PortHint, chunk_of_cell,
        },
        parts::PartFrame,
    },
};
use parble_sim::marble::{MarbleKind, MarbleSize as SimMarbleSize};
use parble_sim::mechanism::MechanismRegistry;
#[cfg(test)]
use parble_sim::track_run::TrackRun;
use parble_sim::track_run::{BuildStage, TrackRunBuild};
use serde::{Deserialize, Serialize};

use crate::SimClock;
use crate::bridge::{self, Event};
use crate::playback::Playback;

mod check;
mod nudge;
pub use check::CheckDiag;
pub use nudge::NudgeDiag;

/// Largest `fill` (and `--editor-fill`).
pub const MAX_FILL: u32 = 4_000;
/// The highest level `]` slices to. The slice is also the ground plane new pieces land on, so
/// past the top piece `]` raises the build level (web: `BUILD_LEVEL_MAX` in `web/ui/src/editor/model.ts`).
pub const BUILD_LEVEL_MAX: i32 = 15;
/// Collider rebuild time per frame. A rebuild stage that starts inside it always finishes, so a
/// frame's rebuild is at most this plus one stage (gather, simplify, tag or install of one chunk).
pub const COLLIDER_BUDGET_MS: f32 = 4.0;
/// Run build time per frame ([`TrackRunBuild`]), a ceiling: a unit starts only while the longest
/// unit so far still fits under it. The longest units (a dense chunk's trimesh) are about 6 ms in
/// GPU Chromium, so a frame does 4 to 10 ms of build work (measured 2026-11-09, P3-T5). The
/// collider rebuild waits while a build runs.
pub const RUN_BUILD_BUDGET_MS: f32 = 10.0;
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
/// Most cells one frame of a drag-to-lay stroke walks (a cursor near the horizon jumps far).
const STROKE_MAX_STEP: i32 = 64;
/// Stroke outlines listed in the diag at most.
const MAX_STROKE_OUTLINES: usize = 64;
/// One half of a Show pulse: the selection mark is off, then on, twice ([`Action::Pulse`]).
const PULSE_PHASE_MS: u128 = 160;

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
    /// Ground columns under the hovered piece and the selection on or off (on at start).
    Columns {
        on: bool,
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
    /// Drag-to-lay. `start` opens a stroke (the web drawer's tile press; a left drag in the place
    /// tool opens one by itself): while it is open, each cell the cursor enters on the stroke's
    /// level gets the brush, one placement per cell. `end` folds the stroke into one undo step;
    /// `cancel` removes what it laid.
    Stroke {
        phase: StrokePhase,
    },
    /// Blink the selection mark on `anchor` twice (the overlay's Show), on real time.
    Pulse {
        anchor: [i32; 3],
    },
    /// Set one piece option of the single selected piece ([`piece_options`]), one undoable
    /// command. The run reads it when it is built, so it applies from the next `run` or `reset`.
    MechConfig {
        key: String,
        value: i64,
    },
    /// The marble check ([`check`]): run the release `run` would make with these arguments once
    /// per marble spec, headless and over frames, at the queue's sizes or with `allSizes` at all
    /// three. Results land in `check`; an edit makes them stale.
    Check {
        release: String,
        #[serde(default)]
        interval_ticks: Option<u32>,
        #[serde(default)]
        all_sizes: bool,
    },
    /// Add a nudge zone ([`nudge`]) over the cell box from `cells[0]` to `cells[1]`, else over the
    /// selected pieces' cells, heading `yawMdeg` (0, +X, by default). It becomes the selection.
    NudgeAdd {
        #[serde(default)]
        cells: Option<[[i32; 3]; 2]>,
        #[serde(default)]
        yaw_mdeg: Option<i32>,
    },
    /// Select nudge zone `index` (the placement selection clears), or none with `null`.
    NudgeSelect {
        index: Option<u32>,
    },
    /// Replace nudge zone `index` (the selected one by default) with `zone`, in its `.parble`
    /// shape (snake_case fields). One undoable command.
    NudgeSet {
        #[serde(default)]
        index: Option<u32>,
        zone: NudgeZone,
    },
    /// Delete nudge zone `index` (the selected one by default).
    NudgeRemove {
        #[serde(default)]
        index: Option<u32>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StrokePhase {
    Start,
    End,
    Cancel,
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
    /// The brush footprint's box at `target`: 8 corners in CSS px (corner `i` takes the high x,
    /// y and z for bits 0, 1 and 2), or `null` off screen. Updated every frame.
    pub outline: Option<Vec<[f32; 2]>>,
}

/// A drag-to-lay stroke in progress.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StrokeDiag {
    pub placed: usize,
    /// The level it lays on, once its first cell is known.
    pub level: Option<i32>,
    /// Outlines (as [`HoverDiag::outline`]) of the first 64 pieces it laid.
    pub outlines: Vec<Vec<[f32; 2]>>,
}

/// The last `run`, `stop` or `reset`: building the run over frames and installing it as the live
/// sim, ms.
#[derive(Debug, Clone, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RunTiming {
    pub op: &'static str,
    /// Build work summed over its frames, with the install.
    pub build_ms: f32,
    pub pieces: usize,
    /// Frames the build spread over.
    pub frames: usize,
    /// Longest frame of build work (the last one includes the install).
    pub frame_ms_max: f32,
    /// Work by stage: binding and placing pieces, chunk colliders, mechanism instances.
    pub place_ms: f32,
    pub collider_ms: f32,
    pub mechanism_ms: f32,
    /// Longest single build unit.
    pub unit_ms_max: f32,
    /// Builds started over by an edit while building.
    pub restarts: usize,
    /// Press to install, wall time.
    pub wall_ms: f32,
}

/// A piece option the inspector offers: its value and its choices.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PieceOption {
    pub key: &'static str,
    pub value: i64,
    pub choices: Vec<OptionChoice>,
}

/// One choice of a [`PieceOption`]: a side choice carries the world `face` it sends the marble
/// to, a named choice (the splitter's mode) its `label`.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct OptionChoice {
    pub value: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub face: Option<PortFace>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<&'static str>,
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
    /// Highest anchor level placed (0 when empty), for the slice stepper.
    pub max_level: i32,
    /// Validation of the track with its hopper resolved, as of the last edit.
    pub issue_count: usize,
    /// The first 32 issues.
    pub issues: Vec<IssueDiag>,
    /// The selection's make-up; `null` when nothing is selected.
    pub selection_info: Option<SelectionDiag>,
    /// Placed pieces by id (the drawer's counts), as of the last edit.
    pub piece_counts: BTreeMap<String, usize>,
    pub stroke: Option<StrokeDiag>,
    /// The anchor a Show pulse is blinking, while it runs.
    pub pulse: Option<[i32; 3]>,
    pub last_run: Option<RunTiming>,
    /// The marble check (`check`): warnings parallel to `issues`, never errors.
    pub check: CheckDiag,
    /// Nudge zones ([`nudge`]): the count, the selected one and each one projected, every frame.
    pub nudges: NudgeDiag,
    pub columns: ColumnsDiag,
    /// What attaches to the one selected piece ([`attach_fits`]); `null` unless exactly one piece
    /// is selected. Computed when the selection or the track changes.
    pub attach: Option<Vec<AttachFit>>,
}

/// A palette piece that attaches to the selected piece, and the first placement that does.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachFit {
    pub piece: &'static str,
    pub anchor: [i32; 3],
    pub yaw: u8,
    pub mirror: bool,
}

/// Every palette piece that can be placed across one of the selected piece's open ports so that
/// the two ports link ([`link_fault`] finds nothing) and [`Editor::check`] accepts the placement,
/// with the first such placement (quarter turns 0 to 3, then mirrored for a chiral piece). `None`
/// unless exactly one piece is selected.
fn attach_fits(st: &EditState) -> Option<Vec<AttachFit>> {
    let i = match st.editor.selection.iter().collect::<Vec<_>>()[..] {
        [i] => i,
        _ => return None,
    };
    let p = st.editor.track().placements.get(i as usize)?;
    let def = st.kit.get(&p.piece)?;
    let open: Vec<_> = def
        .ports
        .iter()
        .map(|port| {
            let mut wp = port.transformed(p.orientation());
            wp.cell = wp.cell + p.anchor;
            wp
        })
        .filter(|wp| !st.occupancy.contains_key(&(wp.cell + wp.face.normal())))
        .collect();
    let mut fits = Vec::new();
    for id in palette() {
        let Some(piece) = piece_index(id).map(pid) else {
            continue;
        };
        let Some(d) = st.kit.get(&piece) else {
            continue;
        };
        let turns = if d.chiral { 8 } else { 4 };
        let fit = (0..turns).find_map(|k| {
            let (yaw, mirror) = (k % 4, k >= 4);
            let o = Orientation::new(Yaw4::new(yaw), mirror_of(mirror));
            d.ports.iter().find_map(|q| {
                let qt = q.transformed(o);
                open.iter().find_map(|wp| {
                    if qt.face != wp.face.opposite() {
                        return None;
                    }
                    let across = wp.cell + wp.face.normal();
                    let mut qw = qt;
                    qw.cell = across;
                    if link_fault(wp, &qw).is_some() {
                        return None;
                    }
                    let anchor = CellCoord::new(
                        across.x - qt.cell.x,
                        across.y - qt.cell.y,
                        across.z - qt.cell.z,
                    );
                    let place =
                        Placement::new(piece.clone(), anchor, Yaw4::new(yaw), mirror_of(mirror));
                    st.editor
                        .check(st.kit, EditCommand::Place(place))
                        .is_ok()
                        .then_some(AttachFit {
                            piece: id,
                            anchor: cell_arr(anchor),
                            yaw,
                            mirror,
                        })
                })
            })
        });
        fits.extend(fit);
    }
    Some(fits)
}

/// Ground columns: on or off, and how many cell columns the hovered piece and the selection draw.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ColumnsDiag {
    pub on: bool,
    pub hover: usize,
    pub selected: usize,
}

/// What the ground columns were drawn for: (on, edits, selection, drag offset, selection tint,
/// hovered piece and its tint).
type ColumnsKey = (
    bool,
    u64,
    Selection,
    CellCoord,
    ColumnTint,
    Option<(Placement, ColumnTint)>,
);

/// The lowest cell of each footprint cell column of `hover`, then of `selected` shifted by
/// `offset`, hovered columns first and at most [`MAX_COLUMNS`]. A column the hovered piece has
/// keeps the hover tint; cells below the floor (level < 0) have none.
fn ground_columns<'a>(
    kit: &KitCatalog,
    hover: Option<(&Placement, ColumnTint)>,
    selected: impl IntoIterator<Item = &'a Placement>,
    offset: CellCoord,
    tint: ColumnTint,
) -> Vec<(IVec3, ColumnTint)> {
    let mut lowest: BTreeMap<(i32, i32), (i32, ColumnTint)> = BTreeMap::new();
    let pieces = hover
        .map(|(p, t)| (p, CellCoord::ORIGIN, t))
        .into_iter()
        .chain(selected.into_iter().map(|p| (p, offset, tint)));
    for (p, shift, t) in pieces {
        let Some(def) = kit.get(&p.piece) else {
            continue;
        };
        for c in p.world_cells(def) {
            let (x, y, z) = (c.x + shift.x, c.y + shift.y, c.z + shift.z);
            if y < 0 {
                continue;
            }
            lowest
                .entry((x, z))
                .and_modify(|e| {
                    if e.1 == t && y < e.0 {
                        e.0 = y;
                    }
                })
                .or_insert((y, t));
        }
    }
    let mut out: Vec<_> = lowest
        .into_iter()
        .map(|((x, z), (y, t))| (IVec3::new(x, y, z), t))
        .collect();
    out.sort_by_key(|(_, t)| matches!(t, ColumnTint::Selected | ColumnTint::SelectedBad));
    out.truncate(MAX_COLUMNS);
    out
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
    /// A `run`, `stop` or `reset` is building its sim; the live sim holds still until it is in.
    pub building: bool,
    /// The build's share done, 0 to 1 ([`TrackRunBuild::progress`]).
    pub build_progress: f32,
    /// The op building (`run`, `stop` or `reset`).
    pub build_op: Option<&'static str>,
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
    pub config: BTreeMap<String, i64>,
    pub options: Vec<PieceOption>,
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

/// A drag-to-lay stroke ([`Action::Stroke`]).
#[derive(Debug, Clone, Default)]
struct Stroke {
    /// The press point of a drag in the place tool: its cell is the stroke's first.
    from_px: Option<Vec2>,
    level: Option<i32>,
    last: Option<CellCoord>,
    /// What it laid, in order.
    placed: Vec<Placement>,
    /// `EditorDiag::edits` before its first piece; folding needs every edit since to be its own.
    base: Option<u64>,
}

#[derive(Debug, Clone, Copy)]
enum Press {
    Box,
    Move { anchor: CellCoord },
}

/// A `run`, `stop` or `reset` building its sim over frames.
struct RunBuild {
    build: TrackRunBuild<'static>,
    op: &'static str,
    release: Option<ReleaseMode>,
    /// [`EditorDiag::edits`] the build's track is from; an edit since starts it over.
    edits: u64,
    started: Instant,
    timing: RunTiming,
}

#[derive(Resource)]
struct EditState {
    editor: Editor,
    kit: &'static KitCatalog,
    /// The behaviours runs and live mechanism edits use ([`parble_sim::behaviour::registry`]).
    registry: MechanismRegistry,
    /// The run, stop or reset being built.
    build: Option<RunBuild>,
    /// The live sim was built by a run, stop or reset, so its mechanism instances follow edits.
    live_mechanisms: bool,
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
    marks_key: Option<(u64, usize, Option<u128>)>,
    /// [`SimClock::restarts`] this state is synced to.
    restarts: u64,
    /// The editor's keyboard shortcuts are on (`keys`).
    keys: bool,
    /// Ground columns are drawn (`columns`).
    columns: bool,
    columns_key: Option<ColumnsKey>,
    /// The marble runs release: this type, every queued marble at this size. `None`: the queued
    /// sizes as legacy marbles.
    marble: Option<(MarbleKind, SimMarbleSize)>,
    name: String,
    run: RunState,
    /// Edit count the issues and placement counts were computed at.
    checked_at: Option<u64>,
    /// Edit count and selection `selectionInfo` was computed for.
    selection_at: Option<(u64, Selection)>,
    stroke: Option<Stroke>,
    /// Show pulse: the anchor and when it started.
    pulse: Option<(CellCoord, Instant)>,
    /// The marble check under way.
    check: check::CheckState,
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
            registry: parble_sim::behaviour::registry(),
            build: None,
            live_mechanisms: false,
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
            columns: true,
            columns_key: None,
            marble: None,
            name: UNTITLED.to_owned(),
            run: RunState {
                active: false,
                release: ReleaseMode::default(),
                interval: DEFAULT_INTERVAL_TICKS,
            },
            checked_at: None,
            selection_at: None,
            stroke: None,
            pulse: None,
            check: default(),
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
        self.stroke = None;
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
            if self.live_mechanisms {
                clock.sim.remove_mechanism(*a);
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
            if self.live_mechanisms
                && let Err(e) = clock.sim.add_mechanism(self.kit, &self.registry, p)
            {
                error!("editor: mechanism add {:?}: {e}", p.anchor);
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
    // The live sim pushes with the edited zones from its next tick, as pieces edited mid-run apply.
    ctx.clock
        .sim
        .set_nudge_zones(&st.editor.track().nudge_zones);
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
        Action::Columns { .. } => "columns",
        Action::Marbles { .. } => "marbles",
        Action::Run { .. } => "run",
        Action::Stop {} => "stop",
        Action::Reset {} => "reset",
        Action::Pause { .. } => "pause",
        Action::Follow { .. } => "follow",
        Action::Rename { .. } => "rename",
        Action::Export {} => "export",
        Action::Stroke { .. } => "stroke",
        Action::Pulse { .. } => "pulse",
        Action::MechConfig { .. } => "mechConfig",
        Action::Check { .. } => "check",
        Action::NudgeAdd { .. } => "nudgeAdd",
        Action::NudgeSelect { .. } => "nudgeSelect",
        Action::NudgeSet { .. } => "nudgeSet",
        Action::NudgeRemove { .. } => "nudgeRemove",
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

/// The track a run of `track` builds: with `release`, the resolved hopper releases its queue in
/// that mode; without, the track idles with no marbles.
fn editor_track(track: &Track, release: Option<ReleaseMode>) -> Result<Track, String> {
    Ok(match release {
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
    })
}

/// A fresh run of [`editor_track`] at tick 0, recording its events, built in one go.
#[cfg(test)]
fn editor_run(track: &Track, release: Option<ReleaseMode>) -> Result<TrackRun, String> {
    TrackRun::new(&editor_track(track, release)?)
        .map(crate::recording)
        .map_err(|e| e.to_string())
}

/// The build of a run of `track` releasing `marble`: its type, with every queued marble at its
/// size. `None` is [`TrackRunBuild::new`].
fn run_build(track: &Track, marble: Option<(MarbleKind, SimMarbleSize)>) -> TrackRunBuild<'static> {
    let Some((kind, size)) = marble else {
        return TrackRunBuild::new(track);
    };
    let mut t = track.clone();
    t.hopper.queue.fill(match size {
        SimMarbleSize::Mini => MarbleSize::Mini,
        SimMarbleSize::Standard => MarbleSize::Standard,
        SimMarbleSize::Shooter => MarbleSize::Shooter,
    });
    TrackRunBuild::with_mechanisms(
        &t,
        parble_kit::builtin(),
        &parble_sim::behaviour::registry(),
        Some(kind),
    )
}

/// Start building a fresh run of the editor's track to replace the live sim, which holds still
/// until [`step_build`] installs it. A build already under way is dropped.
fn start_run(
    st: &mut EditState,
    ctx: &mut Ctx,
    op: &'static str,
    release: Option<ReleaseMode>,
) -> Result<(), String> {
    let track = editor_track(st.editor.track(), release)?;
    st.build = Some(RunBuild {
        build: run_build(&track, st.marble),
        op,
        release,
        edits: ctx.diag.edits,
        started: Instant::now(),
        timing: RunTiming {
            op,
            pieces: track.placements.len(),
            ..default()
        },
    });
    ctx.clock.hold = true;
    st.pending = None;
    st.pending_is_fill = false;
    Ok(())
}

/// Advance the run build by up to `budget_ms` of work (all of it with `None`), then install it once
/// done. A unit starts only while the longest unit so far still fits the budget. An edit since the
/// build started starts it over on the edited track.
fn step_build(
    st: &mut EditState,
    clock: &mut SimClock,
    diag: &mut EditorDiag,
    frame: &mut MarbleFrame,
    parts: &mut PartFrame,
    budget_ms: Option<f32>,
) {
    let Some(b) = st.build.as_mut() else {
        return;
    };
    if b.edits != diag.edits {
        match editor_track(st.editor.track(), b.release) {
            Ok(track) => {
                b.build = run_build(&track, st.marble);
                b.edits = diag.edits;
                b.timing.pieces = track.placements.len();
                b.timing.restarts += 1;
            }
            Err(e) => {
                error!("editor: {}: {e}", b.op);
                st.build = None;
                clock.hold = false;
                return;
            }
        }
    }
    let start = Instant::now();
    let ms = |t: Instant| t.elapsed().as_secs_f32() * 1e3;
    let mut done = b.build.is_done();
    let mut units = 0;
    while !done {
        if units > 0 && budget_ms.is_some_and(|budget| ms(start) + b.timing.unit_ms_max > budget) {
            break;
        }
        units += 1;
        let stage = b.build.stage();
        let one = Instant::now();
        match b.build.step() {
            Ok(d) => done = d,
            Err(e) => {
                error!("editor: {} build: {e}", b.op);
                st.build = None;
                clock.hold = false;
                return;
            }
        }
        let unit = ms(one);
        b.timing.unit_ms_max = b.timing.unit_ms_max.max(unit);
        match stage {
            BuildStage::Place(_) => b.timing.place_ms += unit,
            BuildStage::Colliders => b.timing.collider_ms += unit,
            BuildStage::Mechanisms(_) | BuildStage::Done => b.timing.mechanism_ms += unit,
        }
    }
    b.timing.frames += 1;
    if done {
        let b = st.build.take().expect("the build is under way");
        let mut timing = b.timing;
        match b.build.finish() {
            Ok(run) => {
                clock.install(crate::recording(run), frame, parts);
                st.run.active = b.release.is_some();
                st.live_mechanisms = true;
            }
            Err(e) => error!("editor: {} build: {e}", b.op),
        }
        clock.hold = false;
        let work = ms(start);
        timing.build_ms += work;
        timing.frame_ms_max = timing.frame_ms_max.max(work);
        timing.wall_ms = ms(b.started);
        diag.last_run = Some(timing);
    } else {
        let work = ms(start);
        b.timing.build_ms += work;
        b.timing.frame_ms_max = b.timing.frame_ms_max.max(work);
    }
}

/// [`step_build`] inside [`RUN_BUILD_BUDGET_MS`], once a frame.
fn build_run(
    mut st: ResMut<EditState>,
    mut clock: ResMut<SimClock>,
    mut diag: ResMut<EditorDiag>,
    mut frame: ResMut<MarbleFrame>,
    mut parts: ResMut<PartFrame>,
) {
    step_build(
        &mut st,
        &mut clock,
        &mut diag,
        &mut frame,
        &mut parts,
        Some(RUN_BUILD_BUDGET_MS),
    );
    let b = st.build.as_ref();
    diag.run.building = b.is_some();
    diag.run.build_progress = b.map_or(0.0, |b| b.build.progress());
    diag.run.build_op = b.map(|b| b.op);
}

/// [`check::step`] inside [`check::CHECK_BUDGET_MS`], once a frame. A run, stop or reset building
/// its sim has the frame to itself.
fn step_check(mut st: ResMut<EditState>, mut diag: ResMut<EditorDiag>) {
    if st.build.is_none() {
        check::step(&mut st, &mut diag, Some(check::CHECK_BUDGET_MS));
    }
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
            config: first.config.clone(),
            options: piece_options(first),
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
        Action::Move { offset } if nudge::sole(st).is_some() => {
            let i = nudge::sole(st).expect("guarded");
            nudge::shift(st, ctx, i, arr_cell(offset))?;
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
        Action::Rotate { by } if st.tool != Tool::Place && nudge::sole(st).is_some() => {
            let i = nudge::sole(st).expect("guarded");
            nudge::turn(st, ctx, i, by)?;
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
        Action::Remove {} if nudge::sole(st).is_some() => nudge::remove(st, ctx, None)?,
        Action::Remove {} => {
            let selection = st.editor.selection.clone();
            run_command(st, ctx, "remove", EditCommand::Remove(selection))?;
        }
        Action::ClearSelection {} => {
            st.editor.selection.clear();
            st.editor.nudge = None;
        }
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
        Action::Columns { on } => st.columns = on,
        Action::Marbles { count } => {
            let hopper = marbles_hopper(st.editor.track(), count)?;
            run_command(st, ctx, "marbles", EditCommand::SetHopper(hopper))?;
        }
        Action::Run {
            release,
            interval_ticks,
        } => {
            let release = release_mode(&release, interval_ticks)?;
            start_run(st, ctx, "run", Some(release))?;
            st.run.set_release(release);
            ctx.playback.paused = false;
            ctx.playback.step = false;
        }
        Action::Check {
            release,
            interval_ticks,
            all_sizes,
        } => check::start_check(st, ctx.diag, &release, interval_ticks, all_sizes)?,
        Action::Stop {} => start_run(st, ctx, "stop", None)?,
        Action::Reset {} => {
            let release = match &st.build {
                Some(b) => b.release,
                None => st.run.active.then_some(st.run.release),
            };
            start_run(st, ctx, "reset", release)?;
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
        Action::Stroke { phase } => match phase {
            StrokePhase::Start => {
                st.tool = Tool::Place;
                st.stroke = Some(Stroke::default());
            }
            StrokePhase::End => end_stroke(st, ctx)?,
            StrokePhase::Cancel => cancel_stroke(st, ctx),
        },
        Action::Pulse { anchor } => st.pulse = Some((arr_cell(anchor), Instant::now())),
        Action::MechConfig { key, value } => {
            let index = match st.editor.selection.iter().collect::<Vec<_>>()[..] {
                [i] => i,
                _ => return Err("mechConfig: select one piece".into()),
            };
            let p = st
                .editor
                .track()
                .placements
                .get(index as usize)
                .ok_or("mechConfig: no such placement")?;
            let offered = piece_options(p)
                .into_iter()
                .any(|o| o.key == key && o.choices.iter().any(|c| c.value == value));
            if !offered {
                return Err(format!(
                    "mechConfig: {} has no option {key} = {value}",
                    p.piece.as_str()
                ));
            }
            let mut config = p.config.clone();
            config.insert(key, value);
            run_command(
                st,
                ctx,
                "mechConfig",
                EditCommand::SetMechConfig { index, config },
            )?;
        }
        Action::NudgeAdd { cells, yaw_mdeg } => nudge::add(st, ctx, cells, yaw_mdeg)?,
        Action::NudgeSelect { index } => nudge::select(st, index)?,
        Action::NudgeSet { index, zone } => nudge::set(st, ctx, index, zone)?,
        Action::NudgeRemove { index } => nudge::remove(st, ctx, index)?,
    }
    out.ok = true;
    Ok(out)
}

/// The options the inspector offers for `p`: the toggle's first side (`first`, 0 = the piece's +z
/// side, 1 = its −z side), named by the world face each sends to; the splitter's mode (`mode`,
/// Random or Alternate) and, in Alternate mode, its first side.
pub fn piece_options(p: &Placement) -> Vec<PieceOption> {
    use parble_sim::behaviour::{
        SPLITTER, SPLITTER_ALTERNATE, SPLITTER_MODE, SPLITTER_RANDOM, TOGGLE,
    };
    let first_side = || {
        let o = Orientation::new(p.yaw, p.mirror);
        let face = |z: i32| PortFace::from_normal(o.apply_cell(CellCoord::new(0, 0, z)));
        let (a, b) = (face(1)?, face(-1)?);
        let side = |value, face| OptionChoice {
            value,
            face: Some(face),
            label: None,
        };
        Some(PieceOption {
            key: "first",
            value: p.config.get("first").copied().unwrap_or(0).clamp(0, 1),
            choices: vec![side(0, a), side(1, b)],
        })
    };
    match p.piece.as_str() {
        TOGGLE => first_side().into_iter().collect(),
        SPLITTER => {
            let alternate = p.config.get(SPLITTER_MODE) == Some(&SPLITTER_ALTERNATE);
            let named = |value, label| OptionChoice {
                value,
                face: None,
                label: Some(label),
            };
            let mut opts = vec![PieceOption {
                key: SPLITTER_MODE,
                value: if alternate {
                    SPLITTER_ALTERNATE
                } else {
                    SPLITTER_RANDOM
                },
                choices: vec![
                    named(SPLITTER_RANDOM, "Random"),
                    named(SPLITTER_ALTERNATE, "Alternate"),
                ],
            }];
            if alternate {
                opts.extend(first_side());
            }
            opts
        }
        _ => Vec::new(),
    }
}

/// The cells a 4-connected walk on `a`'s level visits from `a` to `b`, `a` excluded.
fn stroke_walk(a: CellCoord, b: CellCoord) -> Vec<CellCoord> {
    let (dx, dz) = ((b.x - a.x).abs(), (b.z - a.z).abs());
    if dx + dz > STROKE_MAX_STEP {
        return vec![CellCoord::new(b.x, a.y, b.z)];
    }
    let (sx, sz) = ((b.x - a.x).signum(), (b.z - a.z).signum());
    let (mut x, mut z) = (a.x, a.z);
    let mut out = Vec::new();
    while (x, z) != (b.x, b.z) {
        // Step the axis whose share of the way would be smaller after the step.
        let px = i64::from((x - a.x).abs() + 1) * i64::from(dz);
        let pz = i64::from((z - a.z).abs() + 1) * i64::from(dx);
        if x != b.x && (z == b.z || px <= pz) {
            x += sx;
        } else {
            z += sz;
        }
        out.push(CellCoord::new(x, a.y, z));
    }
    out
}

/// Advance the open stroke to `cell` (on its level): lay the brush on every cell entered on the
/// way that takes it, skipping taken ones.
fn stroke_to(st: &mut EditState, ctx: &mut Ctx, cell: CellCoord) {
    let Some(mut s) = st.stroke.take() else {
        return;
    };
    let level = *s.level.get_or_insert(cell.y);
    let cell = CellCoord::new(cell.x, level, cell.z);
    s.base.get_or_insert(ctx.diag.edits);
    let path = match s.last {
        None => vec![cell],
        Some(last) if last == cell => Vec::new(),
        Some(last) => stroke_walk(last, cell),
    };
    for c in path {
        let p = st.placement_at(c);
        if st
            .editor
            .check(st.kit, EditCommand::Place(p.clone()))
            .is_ok()
            && run_command(st, ctx, "place", EditCommand::Place(p.clone())).is_ok()
        {
            s.placed.push(p);
        }
        s.last = Some(c);
    }
    st.stroke = Some(s);
}

/// This frame's step of the open stroke: its first cell is the pick under the press point (or the
/// cursor), later ones the cursor's cell on the stroke's level.
fn step_stroke(st: &mut EditState, ctx: &mut Ctx, cursor: Option<Vec2>) {
    let Some(s) = st.stroke.as_ref() else {
        return;
    };
    let cell = match s.level {
        None => s
            .from_px
            .or(cursor)
            .and_then(|px| ctx.ray(px))
            .and_then(|r| st.pick_ray(r))
            .and_then(Hit::target),
        Some(level) => cursor
            .and_then(|c| ctx.ray(c))
            .and_then(|r| plane_cell(r.origin, *r.direction, level)),
    };
    if let Some(c) = cell {
        stroke_to(st, ctx, c);
    }
}

/// Whether every edit since the stroke opened is one of its own placements.
fn stroke_owns_history(s: &Stroke, ctx: &Ctx) -> bool {
    s.base.map(|b| b + s.placed.len() as u64) == Some(ctx.diag.edits)
}

/// Close the stroke, folding its placements into one undo step (a `PasteGroup` of the same
/// placements, so the track and the colliders are unchanged).
fn end_stroke(st: &mut EditState, ctx: &mut Ctx) -> Result<(), String> {
    let Some(s) = st.stroke.take() else {
        return Ok(());
    };
    let n = s.placed.len();
    if n < 2 || !stroke_owns_history(&s, ctx) {
        return Ok(());
    }
    let start = Instant::now();
    for _ in 0..n {
        st.editor.undo();
    }
    let paste = EditCommand::PasteGroup {
        placements: s.placed,
        offset: CellCoord::ORIGIN,
    };
    if let Err(e) = st.editor.apply(st.kit, paste) {
        for _ in 0..n {
            st.editor.redo();
        }
        return Err(format!("stroke: {e}"));
    }
    finish_edit(st, ctx, "lay", start);
    Ok(())
}

/// Close the stroke and take back what it laid.
fn cancel_stroke(st: &mut EditState, ctx: &mut Ctx) {
    let Some(s) = st.stroke.take() else {
        return;
    };
    if s.placed.is_empty() || !stroke_owns_history(&s, ctx) {
        return;
    }
    let start = Instant::now();
    for _ in 0..s.placed.len() {
        st.editor.undo();
    }
    finish_edit(st, ctx, "strokeCancel", start);
}

/// The axis box (metres) around `p`'s footprint.
fn footprint_box(kit: &KitCatalog, p: &Placement) -> Option<(Vec3, Vec3)> {
    let def = kit.get(&p.piece)?;
    let cells = p.world_cells(def);
    let first = *cells.first()?;
    let (lo, hi) = cells.iter().fold((first, first), |(lo, hi), c| {
        (
            CellCoord::new(lo.x.min(c.x), lo.y.min(c.y), lo.z.min(c.z)),
            CellCoord::new(hi.x.max(c.x), hi.y.max(c.y), hi.z.max(c.z)),
        )
    });
    let half = Vec3::new(CELL_M * 0.5, 0.0, CELL_M * 0.5);
    Some((
        cell_floor_m(lo) - half,
        cell_floor_m(hi) + half + Vec3::Y * CELL_M,
    ))
}

/// The 8 corners of `b` projected by `project` (corner `i`: bit 0 high x, bit 1 high y, bit 2
/// high z), or `None` if any is off the view.
fn project_box(b: (Vec3, Vec3), project: impl Fn(Vec3) -> Option<Vec2>) -> Option<Vec<[f32; 2]>> {
    let (lo, hi) = b;
    (0..8)
        .map(|i| {
            let p = Vec3::new(
                if i & 1 == 0 { lo.x } else { hi.x },
                if i & 2 == 0 { lo.y } else { hi.y },
                if i & 4 == 0 { lo.z } else { hi.z },
            );
            project(p).map(|v| [v.x, v.y])
        })
        .collect()
}

fn click(st: &mut EditState, ctx: &mut Ctx, px: Vec2, shift: bool) -> Result<(), String> {
    let ray = ctx.ray(px);
    let hit = ray.and_then(|r| st.pick_ray(r));
    // A click that hits no piece selects (or deletes) the nudge zone it passes through.
    if matches!(st.tool, Tool::Select | Tool::Delete)
        && !matches!(hit, Some(Hit::Piece { .. }))
        && let Some(i) = ray.and_then(|r| nudge::pick(st, r))
    {
        return if st.tool == Tool::Select {
            nudge::select(st, Some(i))
        } else {
            nudge::remove(st, ctx, Some(i))
        };
    }
    let hit = hit.ok_or("click: the ray hits nothing")?;
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
                Hit::Ground { .. } if !shift => {
                    st.editor.selection.clear();
                    st.editor.nudge = None;
                }
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
    /// The marble every run releases ([`crate::agent::Scenario::editor_pick`]).
    pub marble: Option<(MarbleKind, SimMarbleSize)>,
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
        let mut state = EditState::new();
        state.marble = self.marble;
        app.insert_resource(state)
            .insert_resource(CameraMode::Orbit)
            .insert_resource(EditorCamera {
                active: true,
                set: Some(Orbit {
                    target: Vec3::new(1.2, 0.0, -1.2),
                    yaw: -0.5,
                    pitch: 0.95,
                    radius: 2.6,
                }),
                pivot: None,
            })
            .add_systems(
                Update,
                (
                    mouse_and_keys,
                    apply_actions,
                    build_run,
                    step_check,
                    update_hover,
                    rebuild_colliders,
                    orbit_pivot,
                )
                    .chain(),
            );
    }
}

/// When a right drag starts, sets the orbit pivot: the selection's centre, else the centre of the
/// piece or ground cell under the cursor, else none (the orbit turns about its own target).
fn orbit_pivot(
    st: Res<EditState>,
    buttons: Option<Res<ButtonInput<MouseButton>>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    cams: Query<(&Camera, &GlobalTransform), With<MainCamera>>,
    mut editor_cam: ResMut<EditorCamera>,
) {
    if !buttons.is_some_and(|b| b.just_pressed(MouseButton::Right)) {
        return;
    }
    let placements = &st.editor.track().placements;
    let centre = |ps: &mut dyn Iterator<Item = &Placement>| {
        ps.filter_map(|p| footprint_box(st.kit, p))
            .reduce(|(lo, hi), (l, h)| (lo.min(l), hi.max(h)))
            .map(|(lo, hi)| (lo + hi) * 0.5)
    };
    let selected = centre(
        &mut st
            .editor
            .selection
            .iter()
            .filter_map(|i| placements.get(i as usize)),
    );
    editor_cam.pivot = selected.or_else(|| {
        let (cam, gt) = cams.single().ok()?;
        let px = windows.single().ok()?.cursor_position()?;
        match st.pick_ray(cam.viewport_to_world(gt, px).ok()?)? {
            Hit::Piece { anchor, .. } => {
                centre(&mut placements.iter().filter(|p| p.anchor == anchor))
            }
            Hit::Ground { cell } => Some(cell_floor_m(cell)),
        }
    });
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
            // A drag in the place tool lays a run from the pressed cell.
            if st.tool == Tool::Place && st.stroke.is_none() {
                st.stroke = Some(Stroke {
                    from_px: Some(p0),
                    ..default()
                });
            }
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
            } else if st.tool == Tool::Place {
                out.push(Action::Stroke {
                    phase: StrokePhase::End,
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
                level: Some(
                    st.slice
                        .map_or(max_level + 1, |s| s + 1)
                        .min(BUILD_LEVEL_MAX),
                ),
            }),
            KeyCode::KeyX => out.push(Action::Slice { level: None }),
            KeyCode::KeyH => out.push(Action::Columns { on: !st.columns }),
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
    mut playback: ResMut<Playback>,
    mut mode: ResMut<CameraMode>,
    cams: Query<(&Camera, &GlobalTransform), With<MainCamera>>,
    windows: Query<&Window, With<PrimaryWindow>>,
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
        // The bridge's restart replaced the sim: a build under way is moot, and the restarted
        // sim's mechanisms are the loaded track's, not the editor's.
        st.build = None;
        st.live_mechanisms = false;
        clock.hold = false;
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
    if st.stroke.is_some() {
        if st.tool == Tool::Place && *ctx.mode != CameraMode::Fly {
            let cursor = windows.single().ok().and_then(Window::cursor_position);
            step_stroke(st, &mut ctx, cursor);
        } else {
            st.stroke = None;
        }
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
    // A zone and pieces are never selected together: selecting pieces drops the zone.
    if !st.editor.selection.is_empty() {
        st.editor.nudge = None;
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
    d.columns.on = st.columns;
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
        building: st.build.is_some(),
        build_progress: st.build.as_ref().map_or(0.0, |b| b.build.progress()),
        build_op: st.build.as_ref().map(|b| b.op),
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
        d.max_level = st.by_anchor.keys().map(|a| a.y).max().unwrap_or(0);
        d.piece_counts.clear();
        for p in st.by_anchor.values() {
            *d.piece_counts
                .entry(p.piece.as_str().to_owned())
                .or_insert(0) += 1;
        }
    }
    if st
        .selection_at
        .as_ref()
        .is_none_or(|(e, s)| *e != d.edits || *s != st.editor.selection)
    {
        st.selection_at = Some((d.edits, st.editor.selection.clone()));
        d.selection_info = selection_diag(st);
        d.attach = attach_fits(st);
    }
}

#[allow(clippy::too_many_arguments)]
fn update_hover(
    mut st: ResMut<EditState>,
    mut diag: ResMut<EditorDiag>,
    mut ghost: ResMut<KitGhost>,
    mut marks: ResMut<KitMarks>,
    mut columns: ResMut<KitColumns>,
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
        _ if st.stroke.is_some() => None,
        HoverSrc::Cell(c) => Some(c),
        _ if st.tool == Tool::Place => hit.and_then(Hit::target),
        _ => None,
    };
    let project = |p: Vec3| camera.and_then(|(c, gt)| c.world_to_viewport(gt, p).ok());
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
            outline: None,
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
        h.outline = target
            .and_then(|t| footprint_box(st.kit, &st.placement_at(t)))
            .and_then(|b| project_box(b, project));
    }
    diag.nudges = nudge::diag(&st, project);
    diag.stroke = st.stroke.as_ref().map(|s| StrokeDiag {
        placed: s.placed.len(),
        level: s.level,
        outlines: s
            .placed
            .iter()
            .take(MAX_STROKE_OUTLINES)
            .filter_map(|p| footprint_box(st.kit, p).and_then(|b| project_box(b, project)))
            .collect(),
    });
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
    // Show pulse: the mark on its anchor goes off in phases 0 and 2, on in 1 and 3, then it ends.
    let phase = st.pulse.and_then(|(a, t0)| {
        let n = t0.elapsed().as_millis() / PULSE_PHASE_MS;
        (n < 4).then_some((a, n))
    });
    if phase.is_none() {
        st.pulse = None;
    }
    diag.pulse = phase.map(|(a, _)| cell_arr(a));
    let hidden = phase.filter(|(_, n)| n % 2 == 0).map(|(a, _)| a);
    let mkey = (edits, st.editor.selection.len(), phase.map(|(_, n)| n));
    if st.marks_key != Some(mkey) || drag.is_some() || diag.drag.is_some() {
        st.marks_key = Some(mkey);
        let placements = &st.editor.track().placements;
        marks.cells = st
            .editor
            .selection
            .iter()
            .take(MAX_MARKS)
            .filter_map(|i| placements.get(i as usize).map(|p| p.anchor))
            .filter(|a| Some(*a) != hidden)
            .map(ivec)
            .collect();
        marks.offset = ivec(offset);
        marks.valid = drag.as_ref().is_none_or(|d| d.valid);
    }

    // Ground columns: the ghost (by its validity), else the piece under the cursor when it is not
    // selected (red for the delete tool), and the selection where a move drag would put it.
    let hover_piece = if hover_known {
        None
    } else if let Some(t) = target {
        let tint = if ghost.valid {
            ColumnTint::Hover
        } else {
            ColumnTint::HoverBad
        };
        Some((st.placement_at(t), tint))
    } else {
        match hit {
            Some(Hit::Piece { anchor, .. })
                if st.stroke.is_none()
                    && !st
                        .index_of
                        .get(&anchor)
                        .is_some_and(|&i| st.editor.selection.contains(i)) =>
            {
                let tint = if st.tool == Tool::Delete {
                    ColumnTint::HoverBad
                } else {
                    ColumnTint::Hover
                };
                st.by_anchor.get(&anchor).map(|p| (p.clone(), tint))
            }
            _ => None,
        }
    };
    let sel_tint = if drag.as_ref().is_none_or(|d| d.valid) {
        ColumnTint::Selected
    } else {
        ColumnTint::SelectedBad
    };
    let ckey: ColumnsKey = (
        st.columns,
        edits,
        st.editor.selection.clone(),
        offset,
        sel_tint,
        hover_piece,
    );
    if st.columns_key.as_ref() != Some(&ckey) {
        columns.cells = if st.columns {
            let placements = &st.editor.track().placements;
            ground_columns(
                st.kit,
                ckey.5.as_ref().map(|(p, t)| (p, *t)),
                st.editor
                    .selection
                    .iter()
                    .filter_map(|i| placements.get(i as usize)),
                offset,
                sel_tint,
            )
        } else {
            Vec::new()
        };
        let hovered = columns
            .cells
            .iter()
            .filter(|(_, t)| matches!(t, ColumnTint::Hover | ColumnTint::HoverBad))
            .count();
        diag.columns.hover = hovered;
        diag.columns.selected = columns.cells.len() - hovered;
        st.columns_key = Some(ckey);
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
    // A run build has the frame's budget, and replaces this sim when it is in.
    while st.build.is_none() {
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
    fn ground_columns_take_the_lowest_cell_and_keep_the_hover_tint() {
        let st = EditState::new();
        let kit = st.kit;
        let low = placed("blk_straight_l3", c(0, 2, 0));
        let high = placed("blk_straight_l3", c(0, 5, 0));
        let cells = low.world_cells(kit.get(&low.piece).unwrap());
        // A stacked selection: one column per footprint cell, at the lower piece's level.
        let cols = ground_columns(
            kit,
            None,
            [&high, &low],
            CellCoord::ORIGIN,
            ColumnTint::Selected,
        );
        assert_eq!(cols.len(), cells.len());
        assert!(
            cols.iter()
                .all(|&(c, t)| c.y == 2 && t == ColumnTint::Selected)
        );
        // A move drag shifts the selection's columns; a level below the floor draws none.
        let moved = ground_columns(kit, None, [&low], c(1, -3, 0), ColumnTint::SelectedBad);
        assert!(moved.is_empty());
        let moved = ground_columns(kit, None, [&low], c(1, 0, 0), ColumnTint::SelectedBad);
        assert!(
            moved
                .iter()
                .all(|&(m, _)| cells.iter().any(|o| o.x + 1 == m.x && o.z == m.z))
        );
        // The hovered piece's columns come first and keep their tint where they overlap.
        let cols = ground_columns(
            kit,
            Some((&high, ColumnTint::Hover)),
            [&low],
            CellCoord::ORIGIN,
            ColumnTint::Selected,
        );
        assert_eq!(cols.len(), cells.len());
        assert!(
            cols.iter()
                .all(|&(c, t)| c.y == 5 && t == ColumnTint::Hover)
        );
    }

    #[test]
    fn h_toggles_the_ground_columns() {
        let mut st = EditState::new();
        assert!(st.columns, "columns start on");
        let mut k = ButtonInput::<KeyCode>::default();
        k.press(KeyCode::KeyH);
        assert_eq!(key_actions(&st, &k, false), [Action::Columns { on: false }]);
        st.columns = false;
        assert_eq!(key_actions(&st, &k, false), [Action::Columns { on: true }]);
        assert_eq!(
            parse(r#"{"op":"columns","on":false}"#),
            Action::Columns { on: false }
        );
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
    fn a_picked_marble_runs_at_its_size_and_no_pick_keeps_the_legacy_marble() {
        let mut track = Track::new(0);
        track.placements.push(placed("mech_hopper", c(0, 0, 0)));
        track.hopper.queue = vec![MarbleSize::Standard; 3];
        let track = editor_track(&track, Some(ReleaseMode::All)).unwrap();
        let radius = |pick| {
            let mut run = run_build(&track, pick).finish().unwrap();
            run.tick(1);
            let r = run.marble_radii();
            assert!(!r.is_empty());
            r[0]
        };
        let shooter = SimMarbleSize::Shooter.radius_m();
        assert_eq!(
            radius(Some((MarbleKind::Steel, SimMarbleSize::Shooter))),
            shooter
        );
        let legacy = parble_sim::marble::spec_for(None, SimMarbleSize::Standard).radius_m;
        assert_eq!(radius(None), legacy);
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

    /// The marble check runs the release named in the action once per marble spec, reports a stall
    /// for each at the piece holding it (the end stop, or the straight before it), names the
    /// release, and goes stale on the next edit.
    #[test]
    fn the_marble_check_reports_a_stall_and_goes_stale_on_an_edit() {
        let mut rig = Rig::new();
        let mut st = EditState::new();
        let ctx = &mut rig.ctx();
        let st = &mut st;
        assert_eq!(
            act(st, ctx, r#"{"op":"check","release":"one"}"#).err(),
            Some("check: no hopper".to_owned())
        );
        for (piece, cell) in [
            ("mech_hopper", [4, 1, -6]),
            ("blk_pillar", [4, 0, -6]),
            ("blk_straight_l3", [5, 0, -6]),
            ("blk_end_l3", [6, 0, -6]),
        ] {
            act(
                st,
                ctx,
                &format!(r#"{{"op":"place","piece":"{piece}","cell":{cell:?}}}"#),
            )
            .unwrap();
        }
        act(st, ctx, r#"{"op":"check","release":"one"}"#).unwrap();
        assert_eq!(ctx.diag.check.state, "running");
        let mut frames = 0;
        while ctx.diag.check.state == "running" {
            check::step(st, ctx.diag, Some(check::CHECK_BUDGET_MS));
            frames += 1;
            assert!(frames < 100_000);
        }
        let c = &ctx.diag.check;
        assert_eq!(c.state, "done", "{:?}", c.error);
        assert!(frames > 1, "the check spreads over frames");
        let r = c.release.as_ref().unwrap();
        assert_eq!(
            (r.release, r.marbles, r.sizes.clone(), r.all_sizes),
            ("one", 10, vec!["standard"], false)
        );
        let marbles: Vec<&str> = c.runs.iter().map(|r| r.marble).collect();
        assert_eq!(marbles, ["cats-eye", "glazed", "steel"]);
        assert_eq!(
            c.runs[0].also,
            ["swirl", "onionskin", "opal", "glow"].to_vec()
        );
        assert_eq!(c.warning_count, 3);
        // Every spec bounces back off the end stop (PHYSICS_VERSION 9's tube hand-over) and again
        // off the pillar's wall. Cat's eye and steel roll back into the end stop and rest there;
        // glazed comes back off the pillar at about 0.03 m/s and rests on the straight at
        // x = 0.310 m, 20 mm short of the end stop's cell, so the stall is the straight's.
        let at: Vec<_> = c
            .warnings
            .iter()
            .map(|w| (w.marble, w.piece.as_deref(), w.anchor))
            .collect();
        assert_eq!(
            at,
            [
                ("cats-eye", Some("blk_end_l3"), Some([6, 0, -6])),
                ("glazed", Some("blk_straight_l3"), Some([5, 0, -6])),
                ("steel", Some("blk_end_l3"), Some([6, 0, -6])),
            ]
        );
        for w in &c.warnings {
            assert_eq!(w.kind, "stalled");
            assert_eq!(w.marbles, [0]);
        }
        // An edit leaves the warnings listed, marked stale.
        act(
            st,
            ctx,
            r#"{"op":"place","piece":"blk_straight_l3","cell":[5,0,-4]}"#,
        )
        .unwrap();
        check::step(st, ctx.diag, None);
        assert_eq!(ctx.diag.check.state, "stale");
        assert_eq!(ctx.diag.check.warning_count, 3);
        // All sizes: three sizes per spec.
        act(
            st,
            ctx,
            r#"{"op":"check","release":"interval","intervalTicks":480,"allSizes":true}"#,
        )
        .unwrap();
        check::step(st, ctx.diag, None);
        let c = &ctx.diag.check;
        assert_eq!(c.state, "done");
        assert_eq!(c.runs.len(), 9);
        let r = c.release.as_ref().unwrap();
        assert_eq!((r.release, r.interval_ticks), ("interval", 480));
    }

    fn run_check(st: &mut EditState, ctx: &mut Ctx) -> usize {
        act(
            st,
            ctx,
            r#"{"op":"check","release":"interval","intervalTicks":480}"#,
        )
        .unwrap();
        check::step(st, ctx.diag, None);
        assert_eq!(ctx.diag.check.state, "done", "{:?}", ctx.diag.check.error);
        ctx.diag.check.warning_count
    }

    /// Nudge zones place, move, aim, change and delete with undo and redo; the live sim gets
    /// them on every edit; export writes them; and the marble check runs with them, so a zone over
    /// the rope arches of the nudge track clears its stall warnings.
    #[test]
    fn nudge_zones_edit_with_undo_reach_the_sim_and_clear_the_check() {
        let mut rig = Rig::new();
        let mut st = EditState::new();
        let ctx = &mut rig.ctx();
        let st = &mut st;
        let track = parble_sim::nudge::flat_track();
        for p in &track.placements {
            act(
                st,
                ctx,
                &format!(
                    r#"{{"op":"place","piece":"{}","cell":[{},{},{}]}}"#,
                    p.piece.as_str(),
                    p.anchor.x,
                    p.anchor.y,
                    p.anchor.z
                ),
            )
            .unwrap();
        }
        let bare = run_check(st, ctx);
        assert!(bare >= 3, "every checked type stalls in the arches: {bare}");
        assert!(
            ctx.diag.check.warnings.iter().all(|w| w.kind == "stalled"),
            "{:?}",
            ctx.diag.check.warnings
        );

        // No selection and no cells: refused. Over the selected run: added and selected.
        act(st, ctx, r#"{"op":"clearSelection"}"#).unwrap();
        assert!(act(st, ctx, r#"{"op":"nudgeAdd"}"#).is_err());
        act(st, ctx, r#"{"op":"boxSelect","a":[1,1,-6],"b":[12,1,-6]}"#).unwrap();
        assert_eq!(st.editor.selection.len(), 12);
        act(st, ctx, r#"{"op":"nudgeAdd"}"#).unwrap();
        let zones = |st: &EditState| st.editor.track().nudge_zones.clone();
        assert_eq!(st.editor.selection.len(), 0, "the zone is the selection");
        assert_eq!(st.editor.nudge, Some(0));
        let z = st.editor.track().nudge_zones[0].clone();
        assert_eq!(z, NudgeZone::over_cells(c(1, 1, -6), c(12, 1, -6), 0));
        assert_eq!(ctx.clock.sim.nudge_count(), 1, "the live sim has it");
        assert_eq!(run_check(st, ctx), 0, "the zone clears every stall");

        // Move, aim and set, one undo step each.
        act(st, ctx, r#"{"op":"move","offset":[0,0,1]}"#).unwrap();
        // The place tool's rotate turns the brush; the select tool's turns the zone.
        act(st, ctx, r#"{"op":"rotate","by":1}"#).unwrap();
        assert_eq!(st.editor.track().nudge_zones[0].yaw_mdeg, 0);
        act(st, ctx, r#"{"op":"tool","tool":"select"}"#).unwrap();
        act(st, ctx, r#"{"op":"rotate","by":3}"#).unwrap();
        let moved = &st.editor.track().nudge_zones[0];
        assert_eq!(moved.min_um[2], z.min_um[2] + 60_000);
        assert_eq!(moved.yaw_mdeg, 270_000);
        let mut fast = moved.clone();
        fast.min_speed_um_s = 300_000;
        fast.pitch_mdeg = 5_000;
        act(
            st,
            ctx,
            &format!(
                r#"{{"op":"nudgeSet","zone":{}}}"#,
                serde_json::to_string(&fast).unwrap()
            ),
        )
        .unwrap();
        assert_eq!(zones(st), vec![fast.clone()]);
        let mut bad = fast.clone();
        bad.max_accel_um_s2 = 3_000_000;
        assert!(
            act(
                st,
                ctx,
                &format!(
                    r#"{{"op":"nudgeSet","zone":{}}}"#,
                    serde_json::to_string(&bad).unwrap()
                ),
            )
            .is_err()
        );
        assert_eq!(zones(st), vec![fast.clone()]);
        let json = export_json(st.editor.track(), "nudged").unwrap();
        let file = parble_core::file::load(&json).unwrap();
        assert_eq!(file.track.nudge_zones, vec![fast.clone()]);

        // Undo walks back set, turn, move; redo replays them.
        for _ in 0..3 {
            act(st, ctx, r#"{"op":"undo"}"#).unwrap();
        }
        assert_eq!(zones(st), vec![z.clone()]);
        assert_eq!(st.editor.nudge, None, "undo clears the zone selection");
        for _ in 0..3 {
            act(st, ctx, r#"{"op":"redo"}"#).unwrap();
        }
        assert_eq!(zones(st), vec![fast.clone()]);

        // Selecting pieces drops the zone; selecting the zone drops the pieces; remove deletes it.
        act(st, ctx, r#"{"op":"nudgeSelect","index":0}"#).unwrap();
        assert!(act(st, ctx, r#"{"op":"nudgeSelect","index":1}"#).is_err());
        act(st, ctx, r#"{"op":"remove"}"#).unwrap();
        assert!(zones(st).is_empty());
        assert_eq!(ctx.clock.sim.nudge_count(), 0);
        act(st, ctx, r#"{"op":"undo"}"#).unwrap();
        assert_eq!(zones(st), vec![fast]);
        assert_eq!(ctx.clock.sim.nudge_count(), 1);
        act(
            st,
            ctx,
            r#"{"op":"nudgeAdd","cells":[[0,2,0],[1,2,0]],"yawMdeg":-90000}"#,
        )
        .unwrap();
        assert_eq!(st.editor.track().nudge_zones[1].yaw_mdeg, 270_000);
        act(st, ctx, r#"{"op":"nudgeRemove","index":0}"#).unwrap();
        assert_eq!(zones(st).len(), 1);
        assert_eq!(ctx.clock.sim.nudge_count(), 1);
    }

    /// Finish the run build under way, if any, and install it ([`step_build`] with no budget).
    fn finish_build(st: &mut EditState, ctx: &mut Ctx) {
        step_build(
            st,
            ctx.clock,
            ctx.diag,
            ctx.frame,
            &mut PartFrame::default(),
            None,
        );
        assert!(st.build.is_none());
    }

    #[test]
    fn run_ops_swap_the_sim_and_keep_the_edits_and_undo() {
        let mut st = EditState::new();
        let mut clock = SimClock {
            sim: TrackRun::from_sim(parble_sim::Sim::track_floor_scene()),
            scenario: crate::agent::Scenario::Editor,
            tracks: Default::default(),
            marble: None,
            marble_size: parble_sim::marble::MarbleSize::Standard,
            acc: 0.0,
            runs: 0,
            last_run_hash: None,
            restarts: 0,
            hold: false,
        };
        let (mut edits, mut diag, mut editor_cam) = Default::default();
        let (mut frame, mut playback) = Default::default();
        let mut mode = CameraMode::Orbit;
        let ctx = &mut Ctx {
            clock: &mut clock,
            edits: &mut edits,
            diag: &mut diag,
            camera: None,
            editor_cam: &mut editor_cam,
            frame: &mut frame,
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
        // The run builds over frames while the live sim holds still.
        assert!(st.build.is_some() && ctx.clock.hold && !st.run.active);
        finish_build(st, ctx);
        assert!(!ctx.clock.hold);
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
        finish_build(st, ctx);
        assert_eq!(ctx.clock.sim.tick_count(), 0);
        assert_eq!(ctx.clock.sim.chunk_colliders().dirty_chunks().count(), 0);
        assert!(st.run.active && st.editor.can_undo());
        ctx.clock.sim.tick(480);
        assert_eq!(ctx.clock.sim.marbles().len(), 4);
        act(st, ctx, r#"{"op":"stop"}"#).unwrap();
        finish_build(st, ctx);
        assert!(!st.run.active);
        ctx.clock.sim.tick(480);
        assert!(ctx.clock.sim.marbles().is_empty());
        act(st, ctx, r#"{"op":"reset"}"#).unwrap();
        finish_build(st, ctx);
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

    /// The pieces of a test `Ctx` (no camera).
    struct Rig {
        clock: SimClock,
        edits: KitEdits,
        diag: EditorDiag,
        editor_cam: EditorCamera,
        frame: MarbleFrame,
        playback: Playback,
        mode: CameraMode,
    }

    impl Rig {
        fn new() -> Self {
            let (edits, diag, editor_cam) = Default::default();
            let (frame, playback) = Default::default();
            Self {
                clock: SimClock {
                    sim: TrackRun::from_sim(parble_sim::Sim::track_floor_scene()),
                    scenario: crate::agent::Scenario::Editor,
                    tracks: Default::default(),
                    marble: None,
                    marble_size: parble_sim::marble::MarbleSize::Standard,
                    acc: 0.0,
                    runs: 0,
                    last_run_hash: None,
                    restarts: 0,
                    hold: false,
                },
                edits,
                diag,
                editor_cam,
                frame,
                playback,
                mode: CameraMode::Orbit,
            }
        }

        fn ctx(&mut self) -> Ctx<'_> {
            Ctx {
                clock: &mut self.clock,
                edits: &mut self.edits,
                diag: &mut self.diag,
                camera: None,
                editor_cam: &mut self.editor_cam,
                frame: &mut self.frame,
                playback: &mut self.playback,
                mode: &mut self.mode,
            }
        }
    }

    #[test]
    fn a_stroke_walk_enters_every_cell_once_on_the_first_level() {
        let walk = stroke_walk(c(0, 2, 0), c(3, 0, -2));
        assert_eq!(walk.len(), 5);
        assert_eq!(walk.last(), Some(&c(3, 2, -2)));
        for w in std::iter::once(c(0, 2, 0))
            .chain(walk.iter().copied())
            .collect::<Vec<_>>()
            .windows(2)
        {
            assert_eq!(
                (w[1].x - w[0].x).abs() + (w[1].z - w[0].z).abs(),
                1,
                "{w:?}"
            );
            assert_eq!(w[1].y, 2);
        }
        assert!(stroke_walk(c(1, 0, 1), c(1, 0, 1)).is_empty());
        assert_eq!(stroke_walk(c(0, 0, 0), c(100, 0, 0)), vec![c(100, 0, 0)]);
    }

    #[test]
    fn a_stroke_lays_one_piece_per_cell_and_folds_into_one_undo_step() {
        let mut rig = Rig::new();
        let mut st = EditState::new();
        let ctx = &mut rig.ctx();
        let st = &mut st;
        act(
            st,
            ctx,
            r#"{"op":"place","piece":"blk_straight_l3","cell":[2,0,0]}"#,
        )
        .unwrap();
        act(st, ctx, r#"{"op":"stroke","phase":"start"}"#).unwrap();
        // Through [2,0,0], which is taken: it is skipped, the rest are laid.
        stroke_to(st, ctx, c(0, 0, 0));
        stroke_to(st, ctx, c(4, 0, 0));
        stroke_to(st, ctx, c(4, 0, 0));
        // A cell above the stroke's level is laid on its level.
        stroke_to(st, ctx, c(4, 3, -1));
        assert_eq!(st.stroke.as_ref().unwrap().placed.len(), 5);
        assert_eq!(st.editor.track().placements.len(), 6);
        act(st, ctx, r#"{"op":"stroke","phase":"end"}"#).unwrap();
        assert!(st.stroke.is_none());
        let laid: Vec<_> = st
            .editor
            .track()
            .placements
            .iter()
            .map(|p| p.anchor)
            .collect();
        assert_eq!(
            laid,
            vec![
                c(2, 0, 0),
                c(0, 0, 0),
                c(1, 0, 0),
                c(3, 0, 0),
                c(4, 0, 0),
                c(4, 0, -1)
            ]
        );
        assert_eq!(st.by_anchor.len(), 6);
        // One undo takes the whole run back, one redo brings it back with the same order.
        act(st, ctx, r#"{"op":"undo"}"#).unwrap();
        assert_eq!(st.editor.track().placements.len(), 1);
        act(st, ctx, r#"{"op":"redo"}"#).unwrap();
        let again: Vec<_> = st
            .editor
            .track()
            .placements
            .iter()
            .map(|p| p.anchor)
            .collect();
        assert_eq!(again, laid);
        act(st, ctx, r#"{"op":"undo"}"#).unwrap();
        act(st, ctx, r#"{"op":"undo"}"#).unwrap();
        assert!(!st.editor.can_undo());
    }

    #[test]
    fn a_cancelled_stroke_takes_its_pieces_back() {
        let mut rig = Rig::new();
        let mut st = EditState::new();
        let ctx = &mut rig.ctx();
        let st = &mut st;
        act(st, ctx, r#"{"op":"stroke","phase":"start"}"#).unwrap();
        stroke_to(st, ctx, c(0, 0, 0));
        stroke_to(st, ctx, c(0, 0, 3));
        assert_eq!(st.editor.track().placements.len(), 4);
        act(st, ctx, r#"{"op":"stroke","phase":"cancel"}"#).unwrap();
        assert!(st.editor.track().placements.is_empty() && st.by_anchor.is_empty());
        // An empty stroke ends quietly.
        act(st, ctx, r#"{"op":"stroke","phase":"start"}"#).unwrap();
        act(st, ctx, r#"{"op":"stroke","phase":"end"}"#).unwrap();
        assert!(st.editor.track().placements.is_empty());
    }

    #[test]
    fn attach_lists_the_pieces_that_link_to_the_selected_piece() {
        let mut rig = Rig::new();
        let mut st = EditState::new();
        let ctx = &mut rig.ctx();
        let st = &mut st;
        act(
            st,
            ctx,
            r#"{"op":"place","piece":"rail_s_1x3_l2","cell":[0,3,0]}"#,
        )
        .unwrap();
        act(st, ctx, r#"{"op":"clearSelection"}"#).unwrap();
        assert_eq!(attach_fits(st), None, "nothing selected");
        act(st, ctx, r#"{"op":"boxSelect","a":[0,3,0],"b":[0,3,0]}"#).unwrap();
        let fits = attach_fits(st).unwrap();
        let ids: Vec<_> = fits.iter().map(|f| f.piece).collect();
        // The S-bend's ports are channel ports at L2: L2 rails link, L1 rails do not.
        for id in ["rail_straight_l2", "rail_curve90_r90_l2", "rail_s_1x3_l2"] {
            assert!(ids.contains(&id), "{id} fits: {ids:?}");
        }
        assert!(!ids.contains(&"rail_straight_l1"), "{ids:?}");
        // Each fit is a placement the editor takes, and its port then links.
        for f in &fits {
            let i = piece_index(f.piece).unwrap();
            let p = Placement::new(
                pid(i),
                arr_cell(f.anchor),
                Yaw4::new(f.yaw),
                mirror_of(f.mirror),
            );
            assert!(
                st.editor
                    .check(st.kit, EditCommand::Place(p.clone()))
                    .is_ok()
            );
            assert!(
                st.port_hints(&p).iter().any(|h| h.hint == PortHint::Linked),
                "{f:?}"
            );
        }
        // Two selected: no list.
        act(
            st,
            ctx,
            r#"{"op":"place","piece":"rail_straight_l2","cell":[5,3,5]}"#,
        )
        .unwrap();
        act(st, ctx, r#"{"op":"boxSelect","a":[0,3,0],"b":[5,3,5]}"#).unwrap();
        assert_eq!(attach_fits(st), None);
    }

    #[test]
    fn the_toggle_offers_its_first_side_and_keeps_it_through_export() {
        let mut rig = Rig::new();
        let mut st = EditState::new();
        let ctx = &mut rig.ctx();
        let st = &mut st;
        act(
            st,
            ctx,
            r#"{"op":"place","piece":"mech_toggle_l3","cell":[0,0,0],"yaw":1}"#,
        )
        .unwrap();
        let p = &st.editor.track().placements[0];
        let opts = piece_options(p);
        assert_eq!(opts.len(), 1);
        assert_eq!(opts[0].key, "first");
        assert_eq!(opts[0].value, 0);
        // Yaw 1 turns the piece's +z side to +x.
        assert_eq!(opts[0].choices[0].face, Some(PortFace::PosX));
        assert_eq!(opts[0].choices[1].face, Some(PortFace::NegX));
        assert!(piece_options(&placed("blk_straight_l3", c(0, 0, 0))).is_empty());
        act(st, ctx, r#"{"op":"boxSelect","a":[0,0,0],"b":[0,0,0]}"#).unwrap();
        assert!(act(st, ctx, r#"{"op":"mechConfig","key":"first","value":2}"#).is_err());
        assert!(act(st, ctx, r#"{"op":"mechConfig","key":"start","value":1}"#).is_err());
        act(st, ctx, r#"{"op":"mechConfig","key":"first","value":1}"#).unwrap();
        assert_eq!(
            st.editor.track().placements[0].config.get("first"),
            Some(&1)
        );
        let info = selection_diag(st).unwrap().single.unwrap();
        assert_eq!(info.options[0].value, 1);
        let json = export_json(st.editor.track(), "t").unwrap();
        let back = parble_core::file::load(&json).unwrap();
        assert_eq!(back.track.placements[0].config.get("first"), Some(&1));
        act(st, ctx, r#"{"op":"undo"}"#).unwrap();
        assert!(st.editor.track().placements[0].config.is_empty());
    }

    #[test]
    fn the_splitter_offers_random_or_alternate_and_a_first_side_when_alternating() {
        let mut rig = Rig::new();
        let mut st = EditState::new();
        let ctx = &mut rig.ctx();
        let st = &mut st;
        act(
            st,
            ctx,
            r#"{"op":"place","piece":"mech_splitter_l3","cell":[0,0,0],"yaw":1}"#,
        )
        .unwrap();
        // Random, the default: the mode alone, its choices named rather than sided.
        let opts = piece_options(&st.editor.track().placements[0]);
        assert_eq!(opts.len(), 1);
        assert_eq!((opts[0].key, opts[0].value), ("mode", 0));
        let labels: Vec<_> = opts[0].choices.iter().map(|c| (c.value, c.label)).collect();
        assert_eq!(labels, [(0, Some("Random")), (1, Some("Alternate"))]);
        assert!(opts[0].choices.iter().all(|c| c.face.is_none()));
        act(st, ctx, r#"{"op":"boxSelect","a":[0,0,0],"b":[0,0,0]}"#).unwrap();
        // No first side to set while it splits at random, and no third mode.
        assert!(act(st, ctx, r#"{"op":"mechConfig","key":"first","value":1}"#).is_err());
        assert!(act(st, ctx, r#"{"op":"mechConfig","key":"mode","value":2}"#).is_err());
        act(st, ctx, r#"{"op":"mechConfig","key":"mode","value":1}"#).unwrap();
        let info = selection_diag(st).unwrap().single.unwrap();
        assert_eq!(info.options.len(), 2);
        assert_eq!((info.options[0].key, info.options[0].value), ("mode", 1));
        assert_eq!((info.options[1].key, info.options[1].value), ("first", 0));
        // Yaw 1 turns the piece's +z side to +x, as on the toggle.
        assert_eq!(info.options[1].choices[0].face, Some(PortFace::PosX));
        assert_eq!(info.options[1].choices[1].face, Some(PortFace::NegX));
        act(st, ctx, r#"{"op":"mechConfig","key":"first","value":1}"#).unwrap();
        let json = export_json(st.editor.track(), "t").unwrap();
        let back = parble_core::file::load(&json).unwrap();
        let cfg = &back.track.placements[0].config;
        assert_eq!((cfg.get("mode"), cfg.get("first")), (Some(&1), Some(&1)));
        // Back to Random: the first side row goes, the stored side stays for the next switch.
        act(st, ctx, r#"{"op":"mechConfig","key":"mode","value":0}"#).unwrap();
        let info = selection_diag(st).unwrap().single.unwrap();
        assert_eq!(info.options.len(), 1);
        act(st, ctx, r#"{"op":"undo"}"#).unwrap();
        act(st, ctx, r#"{"op":"undo"}"#).unwrap();
        act(st, ctx, r#"{"op":"undo"}"#).unwrap();
        assert!(st.editor.track().placements[0].config.is_empty());
    }

    #[test]
    fn piece_counts_and_run_timing_land_in_the_diag() {
        let mut rig = Rig::new();
        let mut st = EditState::new();
        let ctx = &mut rig.ctx();
        let st = &mut st;
        act(
            st,
            ctx,
            r#"{"op":"place","piece":"mech_hopper","cell":[0,0,0]}"#,
        )
        .unwrap();
        act(st, ctx, r#"{"op":"run","release":"all"}"#).unwrap();
        finish_build(st, ctx);
        let t = ctx.diag.last_run.clone().unwrap();
        assert_eq!((t.op, t.pieces), ("run", 1));
        assert!(t.build_ms >= 0.0);
        act(st, ctx, r#"{"op":"pulse","anchor":[0,0,0]}"#).unwrap();
        assert_eq!(st.pulse.map(|(a, _)| a), Some(c(0, 0, 0)));
    }

    /// P3-T5: a mechanism moved during a run takes effect at its new cell at once: the instance,
    /// its sensors and its part bodies leave the old cell and come up at the new one, the moved
    /// hopper releases the rest of its queue from where it is now, and undoing the move puts the
    /// instance back. A Run builds over frames and an edit while it builds starts it over.
    #[test]
    fn a_mechanism_moved_mid_run_takes_effect_at_its_new_cell() {
        let mut rig = Rig::new();
        let mut st = EditState::new();
        let ctx = &mut rig.ctx();
        let st = &mut st;
        act(
            st,
            ctx,
            r#"{"op":"place","piece":"mech_hopper","cell":[0,0,0]}"#,
        )
        .unwrap();
        act(
            st,
            ctx,
            r#"{"op":"place","piece":"mech_toggle_l3","cell":[6,0,4]}"#,
        )
        .unwrap();
        // Edit mode never grows mechanism instances on the editor's own world.
        assert!(ctx.clock.sim.mechanisms().is_empty());
        act(st, ctx, r#"{"op":"marbles","count":3}"#).unwrap();
        act(
            st,
            ctx,
            r#"{"op":"run","release":"interval","intervalTicks":120}"#,
        )
        .unwrap();
        // An edit while the run builds starts it over on the edited track.
        act(
            st,
            ctx,
            r#"{"op":"place","piece":"blk_straight_l3","cell":[3,0,4]}"#,
        )
        .unwrap();
        finish_build(st, ctx);
        let t = ctx.diag.last_run.clone().unwrap();
        assert_eq!((t.op, t.pieces, t.restarts), ("run", 3, 1));
        assert!(st.run.active);
        let at = |ctx: &Ctx, piece: &str| -> Vec<CellCoord> {
            ctx.clock
                .sim
                .mechanisms()
                .into_iter()
                .filter(|m| m.piece.as_str() == piece)
                .map(|m| m.id.0)
                .collect()
        };
        assert_eq!(at(ctx, "mech_toggle_l3"), [c(6, 0, 4)]);
        ctx.clock.sim.tick(60);
        assert_eq!(ctx.clock.sim.marbles().len(), 1);
        let sensors = |ctx: &Ctx, cell: CellCoord| {
            ctx.clock
                .sim
                .sensors()
                .iter()
                .filter(|s| s.id.0 == cell)
                .count()
        };
        let toggle_sensors = sensors(ctx, c(6, 0, 4));
        assert!(toggle_sensors > 0);

        // Move the toggle two cells along z.
        act(st, ctx, r#"{"op":"boxSelect","a":[6,0,4],"b":[6,0,4]}"#).unwrap();
        act(st, ctx, r#"{"op":"move","offset":[0,0,2]}"#).unwrap();
        assert_eq!(at(ctx, "mech_toggle_l3"), [c(6, 0, 6)]);
        assert_eq!(sensors(ctx, c(6, 0, 4)), 0);
        assert_eq!(sensors(ctx, c(6, 0, 6)), toggle_sensors);

        // Move the hopper: the next marbles come out of its new bowl and none repeats.
        act(st, ctx, r#"{"op":"boxSelect","a":[0,0,0],"b":[0,0,0]}"#).unwrap();
        act(st, ctx, r#"{"op":"move","offset":[-4,0,6]}"#).unwrap();
        assert_eq!(at(ctx, "mech_hopper"), [c(-4, 0, 6)]);
        while ctx
            .clock
            .sim
            .chunk_colliders()
            .dirty_chunks()
            .next()
            .is_some()
        {
            ctx.clock.sim.rebuild_next_chunk(st.kit).unwrap();
        }
        ctx.clock.sim.tick(120);
        let marbles = ctx.clock.sim.marbles();
        assert_eq!(marbles.len(), 2);
        let bowl = c(-4, 0, 6);
        let d = |v: f32, cell: i32| (v - cell as f32 * CELL_M).abs();
        assert!(
            d(marbles[1].position[0], bowl.x) < 2.0 * CELL_M
                && d(marbles[1].position[2], bowl.z) < 2.0 * CELL_M,
            "second marble at {:?}, hopper at {bowl:?}",
            marbles[1].position
        );
        let hopper = ctx
            .clock
            .sim
            .mechanisms()
            .into_iter()
            .find(|m| m.piece.as_str() == "mech_hopper")
            .unwrap();
        assert_eq!(hopper.state["released"], 2);

        // Undo puts the hopper back where it was.
        act(st, ctx, r#"{"op":"undo"}"#).unwrap();
        assert_eq!(at(ctx, "mech_hopper"), [c(0, 0, 0)]);
        // Stop builds the idle sim: the instances follow edits there too.
        act(st, ctx, r#"{"op":"stop"}"#).unwrap();
        finish_build(st, ctx);
        assert!(!st.run.active);
        act(st, ctx, r#"{"op":"boxSelect","a":[0,0,0],"b":[0,0,0]}"#).unwrap();
        act(st, ctx, r#"{"op":"remove"}"#).unwrap();
        assert!(at(ctx, "mech_hopper").is_empty());
        assert_eq!(at(ctx, "mech_toggle_l3"), [c(6, 0, 6)]);
    }

    /// P3-T5 timing note, not a gate: a Run on the 2,000-piece fill built inside
    /// [`RUN_BUILD_BUDGET_MS`] frames, with its stage breakdown. Run with `--nocapture`.
    #[test]
    fn a_run_on_a_2000_piece_fill_builds_over_frames_time() {
        let mut rig = Rig::new();
        let mut st = EditState::new();
        let ctx = &mut rig.ctx();
        let st = &mut st;
        act(st, ctx, r#"{"op":"fill","count":2000}"#).unwrap();
        act(
            st,
            ctx,
            r#"{"op":"place","piece":"mech_hopper","cell":[0,0,0]}"#,
        )
        .unwrap();
        act(st, ctx, r#"{"op":"run","release":"all"}"#).unwrap();
        while st.build.is_some() {
            step_build(
                st,
                ctx.clock,
                ctx.diag,
                ctx.frame,
                &mut PartFrame::default(),
                Some(RUN_BUILD_BUDGET_MS),
            );
        }
        let t = ctx.diag.last_run.clone().unwrap();
        assert!(st.run.active && t.frames > 1);
        println!(
            "run build: {} pieces, {} frames, worst frame {:.2} ms, worst unit {:.2} ms, work {:.0} ms (place {:.1}, colliders {:.0}, mechanisms {:.2})",
            t.pieces,
            t.frames,
            t.frame_ms_max,
            t.unit_ms_max,
            t.build_ms,
            t.place_ms,
            t.collider_ms,
            t.mechanism_ms
        );
    }

    #[test]
    fn a_footprint_box_covers_every_cell_of_the_piece() {
        let kit = parble_kit::builtin();
        let (lo, hi) = footprint_box(kit, &placed("blk_straight_l3", c(2, 1, -3))).unwrap();
        let h = CELL_M * 0.5;
        assert!(
            (lo - Vec3::new(2.0 * CELL_M - h, CELL_M, -3.0 * CELL_M - h)).length() < 1e-6,
            "{lo}"
        );
        assert!(
            (hi - Vec3::new(2.0 * CELL_M + h, 2.0 * CELL_M, -3.0 * CELL_M + h)).length() < 1e-6,
            "{hi}"
        );
        let corners = project_box((lo, hi), |p| Some(Vec2::new(p.x, p.z))).unwrap();
        assert_eq!(corners.len(), 8);
        assert_eq!(corners[0], [lo.x, lo.z]);
        assert_eq!(corners[7], [hi.x, hi.z]);
        assert!(project_box((lo, hi), |_| None).is_none());
    }
}
