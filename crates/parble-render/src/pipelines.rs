//! Render pipeline readiness, read from the render world's [`PipelineCache`] each frame.
//!
//! On wasm Bevy creates every pipeline synchronously on the main thread, inside
//! `PipelineCache::process_queue`: WebGPU's `createRenderPipeline` compiles the WGSL there, so a
//! pipeline first needed mid-run costs one long frame. [`PipelineStats`] counts the cache's
//! pipelines by state and lists the ones created on the last render frame, for the agent diag
//! (`pipelines` in `window.__parble`).
//!
//! [`Warmup`] ends once the kit textures have loaded and the cache has created nothing and has
//! nothing pending for [`QUIET_FRAMES`] render frames. Until then every marble look is drawn off
//! screen (`sync_marbles` in the crate root), so its pipelines are created while loading, not when
//! that marble is first released. READY waits for it. A pipeline still waiting after
//! [`STUCK_FRAMES`] stops holding it, and [`WARMUP_MAX_FRAMES`] ends it regardless, so neither
//! READY nor the parked warm-up marbles can last forever.

use std::sync::{Arc, Mutex, PoisonError};

use bevy::{
    prelude::*,
    render::{
        Render, RenderApp, RenderSystems,
        render_resource::{CachedPipelineState, PipelineCache, PipelineDescriptor},
    },
    shader::{ShaderCacheError, ShaderDefVal},
};
use serde::Serialize;

use crate::kit::KitDiag;

/// Render frames with nothing created or pending, after the textures loaded, that end the warm-up.
/// A texture that finishes loading on frame N has its material prepared and specialized a frame or
/// two later, so this leaves a margin.
pub const QUIET_FRAMES: u64 = 10;

/// Render frames a pipeline may wait for its shader (or a shader import) before the warm-up stops
/// waiting for it. On wasm creation is synchronous, so a pipeline not created on its first frame is
/// waiting for a shader asset. Those load in a few frames (a cold GPU Chromium load creates its
/// last pipeline on render frame 4), so one still waiting after 2 s at 60 Hz is taken as never
/// resolving: it is logged, listed in [`PipelineCounts::waiting`] as `stuck`, and no longer holds
/// [`PipelineCounts::idle`].
pub const STUCK_FRAMES: u64 = 120;

/// The hard ceiling: the warm-up ends on this render frame whatever is still loading or pending,
/// with a warning naming it. 10 s at 60 Hz: 40 times the ~15 render frames a cold GPU Chromium or
/// native load takes, and past [`STUCK_FRAMES`] + [`QUIET_FRAMES`] for a pipeline first queued
/// late. Counted in render frames, not wall time, so a native lockstep run ends it on the same
/// frame whatever the host load.
pub const WARMUP_MAX_FRAMES: u64 = 600;

/// Entries kept in [`PipelineCounts::recent_created`].
const RECENT_CREATED: usize = 24;
/// Entries kept in [`PipelineCounts::waiting`] and [`PipelineCounts::failures`].
const LISTED: usize = 32;

/// A cached pipeline's state as the warm-up reads it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Class {
    Ok,
    /// Queued, being created, or failed with an error the cache retries (a shader or a shader
    /// import not loaded yet). On wasm such a pipeline alternates between `Queued` and that `Err`
    /// from one frame to the next, so both count as waiting.
    Waiting,
    /// A shader error the cache never retries.
    Failed,
}

fn classify_err(e: &ShaderCacheError) -> Class {
    match e {
        ShaderCacheError::ShaderNotLoaded(_) | ShaderCacheError::ShaderImportNotYetAvailable => {
            Class::Waiting
        }
        ShaderCacheError::ProcessShaderError(_) | ShaderCacheError::CreateShaderModule(_) => {
            Class::Failed
        }
    }
}

fn classify(state: &CachedPipelineState) -> (Class, &'static str) {
    match state {
        CachedPipelineState::Ok(_) => (Class::Ok, "ok"),
        CachedPipelineState::Queued => (Class::Waiting, "queued"),
        CachedPipelineState::Creating(_) => (Class::Waiting, "creating"),
        CachedPipelineState::Err(e) => (classify_err(e), "err"),
    }
}

/// A pipeline not created yet.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WaitingPipeline {
    /// Index in the pipeline cache.
    pub index: usize,
    /// `label shader-path [shader defs]`.
    pub name: String,
    /// `queued`, `creating`, or `err` (with `error`: a shader or import the cache retries).
    pub state: &'static str,
    pub error: Option<String>,
    /// The render frame it was first seen waiting.
    pub since_frame: u64,
    /// Waiting [`STUCK_FRAMES`] or more: the warm-up no longer waits for it.
    pub stuck: bool,
}

/// Pipeline cache counts as of the last render frame.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PipelineCounts {
    /// Render frames that have run.
    pub render_frame: u64,
    pub total: usize,
    pub ok: usize,
    /// Not created yet: queued, or waiting for a shader or a shader import to load (an error the
    /// cache retries). On wasm creation is synchronous, so nothing stays `creating`.
    pub pending: usize,
    /// Of `pending`, those waiting [`STUCK_FRAMES`] or more.
    pub stuck: usize,
    /// Failed for good (a shader that does not process or compile).
    pub failed: usize,
    /// The render frame that last created a pipeline.
    pub last_created_frame: u64,
    /// Pipelines created on the last render frame: `label shader-path [shader defs]` (the fragment
    /// shader when the pipeline has one).
    pub created: Vec<String>,
    /// Pipelines created since startup (one recreated after a shader change counts again).
    pub created_total: u64,
    /// The last few creations as `[render frame, name]`, so a poll slower than a frame still sees
    /// pipelines that keep being created.
    pub recent_created: Vec<(u64, String)>,
    /// The pending pipelines (at most 32).
    pub waiting: Vec<WaitingPipeline>,
    /// The failed pipelines as `name: error` (at most 32).
    pub failures: Vec<String>,
}

impl PipelineCounts {
    /// True when the cache has pipelines and none is waiting, bar the stuck ones.
    pub fn idle(&self) -> bool {
        self.total > 0 && self.pending == self.stuck
    }
}

/// Loading warm-up: done (and latched) once the textures have loaded and the pipeline cache has
/// been quiet for [`QUIET_FRAMES`], or on [`WARMUP_MAX_FRAMES`]. A pipeline created later (a look
/// the warm-up did not draw) is still listed in [`PipelineCounts::created`] but does not undo it.
#[derive(Resource, Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Warmup {
    pub done: bool,
    /// The render frame the warm-up ended on.
    pub done_frame: Option<u64>,
    /// `quiet` (the normal end) or `ceiling` ([`WARMUP_MAX_FRAMES`]).
    pub ended_by: Option<&'static str>,
    /// What it waited for on its last frame (kept once done): `textures`, `noPipelines`, `pending`
    /// (a pipeline not stuck yet), `recentCreation` (fewer than [`QUIET_FRAMES`] since the textures
    /// loaded or a pipeline was created), or `nothing`.
    pub waiting_on: &'static str,
    /// Kit textures still loading on its last frame.
    pub textures_loading: usize,
    /// The first render frame of the current run with every texture loaded (or failed).
    pub loaded_frame: Option<u64>,
    /// Pipelines it stopped waiting for ([`STUCK_FRAMES`]), as `name (error)`.
    pub stuck: Vec<String>,
}

impl Warmup {
    fn update(&mut self, counts: &PipelineCounts, textures_loading: usize) {
        if self.done {
            return;
        }
        self.textures_loading = textures_loading;
        self.stuck = counts
            .waiting
            .iter()
            .filter(|w| w.stuck)
            .map(|w| format!("{} ({})", w.name, w.error.as_deref().unwrap_or(w.state)))
            .collect();
        self.waiting_on = if textures_loading > 0 {
            self.loaded_frame = None;
            "textures"
        } else {
            let loaded = *self.loaded_frame.get_or_insert(counts.render_frame);
            if counts.total == 0 {
                "noPipelines"
            } else if !counts.idle() {
                "pending"
            } else if counts.render_frame < counts.last_created_frame.max(loaded) + QUIET_FRAMES {
                "recentCreation"
            } else {
                "nothing"
            }
        };
        if self.waiting_on == "nothing" {
            self.finish(counts.render_frame, "quiet");
            if self.stuck.is_empty() {
                info!(
                    "warm-up done on render frame {}: {} pipelines",
                    counts.render_frame, counts.total
                );
            } else {
                warn!(
                    "warm-up done on render frame {}: {} pipelines, not waiting for {} stuck: {}",
                    counts.render_frame,
                    counts.total,
                    self.stuck.len(),
                    self.stuck.join("; ")
                );
            }
        } else if counts.render_frame >= WARMUP_MAX_FRAMES {
            self.finish(counts.render_frame, "ceiling");
            let names: Vec<&str> = counts.waiting.iter().map(|w| w.name.as_str()).collect();
            warn!(
                "warm-up cut off by the {WARMUP_MAX_FRAMES}-frame ceiling on render frame {}, \
                 waiting on {}: {textures_loading} texture(s) loading, {} of {} pipeline(s) \
                 pending: {}",
                counts.render_frame,
                self.waiting_on,
                counts.pending,
                counts.total,
                names.join("; ")
            );
        }
    }

    fn finish(&mut self, frame: u64, by: &'static str) {
        self.done = true;
        self.done_frame = Some(frame);
        self.ended_by = Some(by);
    }
}

#[derive(Default)]
struct Shared {
    counts: PipelineCounts,
    /// `ok` per cache index on the previous frame, to spot new pipelines.
    was_ok: Vec<bool>,
    /// Per cache index, the render frame it was first seen waiting (`None` once created or failed).
    waiting_since: Vec<Option<u64>>,
}

/// Shared between the main and render worlds.
#[derive(Resource, Clone, Default)]
pub struct PipelineStats(Arc<Mutex<Shared>>);

impl PipelineStats {
    pub fn counts(&self) -> PipelineCounts {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .counts
            .clone()
    }
}

pub struct PipelineStatsPlugin;

impl Plugin for PipelineStatsPlugin {
    fn build(&self, app: &mut App) {
        let stats = PipelineStats::default();
        app.insert_resource(stats.clone())
            .init_resource::<Warmup>()
            .add_systems(PostUpdate, update_warmup);
        if let Some(render_app) = app.get_sub_app_mut(RenderApp) {
            render_app
                .insert_resource(stats)
                .add_systems(Render, count_pipelines.in_set(RenderSystems::Cleanup));
        }
    }
}

fn update_warmup(stats: Res<PipelineStats>, kit: Res<KitDiag>, mut warmup: ResMut<Warmup>) {
    if warmup.done {
        return;
    }
    let loading = kit
        .textures
        .iter()
        .filter(|t| matches!(t.state, "loading" | "notLoaded"))
        .count();
    warmup.update(&stats.counts(), loading);
}

fn describe(d: &PipelineDescriptor) -> String {
    let (label, stage) = match d {
        PipelineDescriptor::RenderPipelineDescriptor(r) => (
            r.label.clone(),
            r.fragment
                .as_ref()
                .map(|f| (&f.shader, &f.shader_defs))
                .unwrap_or((&r.vertex.shader, &r.vertex.shader_defs)),
        ),
        PipelineDescriptor::ComputePipelineDescriptor(c) => {
            (c.label.clone(), (&c.shader, &c.shader_defs))
        }
        PipelineDescriptor::MeshPipelineDescriptor(m) => {
            (m.label.clone(), (&m.mesh.shader, &m.mesh.shader_defs))
        }
    };
    let path = stage
        .0
        .path()
        .map(|p| p.path().to_string_lossy().into_owned())
        .unwrap_or_default();
    let defs: Vec<String> = stage
        .1
        .iter()
        .filter_map(|d| match d {
            ShaderDefVal::Bool(k, true) => Some(k.to_string()),
            ShaderDefVal::Bool(_, false) => None,
            ShaderDefVal::Int(k, v) => Some(format!("{k}={v}")),
            ShaderDefVal::UInt(k, v) => Some(format!("{k}={v}")),
        })
        .collect();
    format!(
        "{} {path} [{}]",
        label.as_deref().unwrap_or("(unlabelled)"),
        defs.join(" ")
    )
}

fn count_pipelines(cache: Res<PipelineCache>, stats: Res<PipelineStats>) {
    let mut shared = stats.0.lock().unwrap_or_else(PoisonError::into_inner);
    let shared = &mut *shared;
    let prev = &shared.counts;
    let frame = prev.render_frame + 1;
    let mut counts = PipelineCounts {
        render_frame: frame,
        last_created_frame: prev.last_created_frame,
        created_total: prev.created_total,
        recent_created: prev.recent_created.clone(),
        ..default()
    };
    for (i, p) in cache.pipelines().enumerate() {
        counts.total += 1;
        if i >= shared.was_ok.len() {
            shared.was_ok.push(false);
            shared.waiting_since.push(None);
        }
        let (class, state) = classify(&p.state);
        match class {
            Class::Ok => {
                counts.ok += 1;
                shared.waiting_since[i] = None;
                if !shared.was_ok[i] {
                    let name = describe(&p.descriptor);
                    counts.recent_created.push((frame, name.clone()));
                    counts.created.push(name);
                }
            }
            Class::Waiting => {
                counts.pending += 1;
                let since = *shared.waiting_since[i].get_or_insert(frame);
                let stuck = frame >= since + STUCK_FRAMES;
                let error = match &p.state {
                    CachedPipelineState::Err(e) => Some(e.to_string()),
                    _ => None,
                };
                if stuck {
                    counts.stuck += 1;
                    if frame == since + STUCK_FRAMES {
                        warn!(
                            "pipeline {i} {} still waiting after {STUCK_FRAMES} render frames ({}); \
                             the warm-up no longer waits for it",
                            describe(&p.descriptor),
                            error.as_deref().unwrap_or(state)
                        );
                    }
                }
                if counts.waiting.len() < LISTED {
                    counts.waiting.push(WaitingPipeline {
                        index: i,
                        name: describe(&p.descriptor),
                        state,
                        error,
                        since_frame: since,
                        stuck,
                    });
                }
            }
            Class::Failed => {
                counts.failed += 1;
                shared.waiting_since[i] = None;
                if counts.failures.len() < LISTED {
                    let e = match &p.state {
                        CachedPipelineState::Err(e) => e.to_string(),
                        _ => String::new(),
                    };
                    counts
                        .failures
                        .push(format!("{}: {e}", describe(&p.descriptor)));
                }
            }
        }
        shared.was_ok[i] = class == Class::Ok;
    }
    if !counts.created.is_empty() {
        counts.last_created_frame = frame;
        counts.created_total += counts.created.len() as u64;
        let excess = counts.recent_created.len().saturating_sub(RECENT_CREATED);
        counts.recent_created.drain(..excess);
        info!(
            "render frame {frame}: {} pipeline(s) created, {} total",
            counts.created.len(),
            counts.total
        );
    }
    shared.counts = counts;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn counts(render_frame: u64, pending: usize, last_created_frame: u64) -> PipelineCounts {
        PipelineCounts {
            render_frame,
            total: 40,
            ok: 40 - pending,
            pending,
            last_created_frame,
            ..default()
        }
    }

    fn stuck(mut c: PipelineCounts, n: usize) -> PipelineCounts {
        c.stuck = n;
        c.waiting = (0..c.pending)
            .map(|i| WaitingPipeline {
                index: i,
                name: format!("p{i}"),
                state: "err",
                error: Some("Shader import not yet available.".into()),
                since_frame: 1,
                stuck: i < n,
            })
            .collect();
        c
    }

    #[test]
    fn warmup_waits_for_quiet_frames_after_the_last_creation_and_latches() {
        let mut w = Warmup::default();
        w.update(&counts(5, 0, 5), 0);
        assert!(!w.done);
        assert_eq!(w.waiting_on, "recentCreation");
        w.update(&counts(5 + QUIET_FRAMES - 1, 0, 5), 0);
        assert!(!w.done);
        w.update(&counts(5 + QUIET_FRAMES, 1, 5), 0);
        assert!(!w.done, "a pending pipeline holds it");
        assert_eq!(w.waiting_on, "pending");
        w.update(&counts(5 + QUIET_FRAMES, 0, 5), 0);
        assert!(w.done);
        assert_eq!(w.done_frame, Some(5 + QUIET_FRAMES));
        assert_eq!(w.ended_by, Some("quiet"));
        w.update(&counts(100, 3, 100), 2);
        assert!(w.done, "latched");
    }

    #[test]
    fn warmup_counts_quiet_frames_from_when_the_textures_loaded() {
        let mut w = Warmup::default();
        w.update(&counts(30, 0, 2), 1);
        assert!(!w.done);
        assert_eq!(w.waiting_on, "textures");
        w.update(&counts(40, 0, 2), 0);
        assert!(
            !w.done,
            "loaded on frame 40: their materials still have to specialize"
        );
        w.update(&counts(40 + QUIET_FRAMES, 0, 2), 0);
        assert!(w.done);
    }

    #[test]
    fn an_empty_cache_is_not_idle() {
        let mut w = Warmup::default();
        let mut c = counts(50, 0, 0);
        c.total = 0;
        c.ok = 0;
        w.update(&c, 0);
        assert!(!w.done);
        assert_eq!(w.waiting_on, "noPipelines");
    }

    #[test]
    fn retryable_shader_errors_are_waiting_and_shader_errors_are_failed() {
        assert_eq!(
            classify_err(&ShaderCacheError::ShaderNotLoaded(AssetId::default())),
            Class::Waiting
        );
        assert_eq!(
            classify_err(&ShaderCacheError::ShaderImportNotYetAvailable),
            Class::Waiting
        );
        assert_eq!(
            classify_err(&ShaderCacheError::ProcessShaderError("x".into())),
            Class::Failed
        );
        assert_eq!(
            classify_err(&ShaderCacheError::CreateShaderModule("x".into())),
            Class::Failed
        );
        assert_eq!(classify(&CachedPipelineState::Queued).0, Class::Waiting);
    }

    #[test]
    fn a_stuck_pipeline_no_longer_holds_the_warmup() {
        let mut w = Warmup::default();
        w.update(&stuck(counts(100, 2, 5), 1), 0);
        assert!(!w.done, "one pending pipeline is not stuck yet");
        w.update(&stuck(counts(140, 2, 5), 2), 0);
        assert!(w.done);
        assert_eq!(w.ended_by, Some("quiet"));
        assert_eq!(w.stuck.len(), 2);
        assert!(w.stuck[0].contains("Shader import not yet available."));
    }

    #[test]
    fn the_ceiling_ends_the_warmup_whatever_is_still_waiting() {
        let mut w = Warmup::default();
        w.update(&counts(WARMUP_MAX_FRAMES - 1, 3, 2), 4);
        assert!(!w.done);
        w.update(&counts(WARMUP_MAX_FRAMES, 3, 2), 4);
        assert!(w.done);
        assert_eq!(w.done_frame, Some(WARMUP_MAX_FRAMES));
        assert_eq!(w.ended_by, Some("ceiling"));
        assert_eq!(w.waiting_on, "textures");
        assert_eq!(w.textures_loading, 4);

        let mut w = Warmup::default();
        w.update(&counts(WARMUP_MAX_FRAMES + 7, 1, 2), 0);
        assert_eq!(
            (w.ended_by, w.waiting_on),
            (Some("ceiling"), "pending"),
            "a pipeline that keeps waiting"
        );
    }

    #[test]
    fn the_ceiling_is_well_above_a_normal_load() {
        // Chromium and native end around render frame 13 to 15; a stuck pipeline queued late must
        // still end quietly before the ceiling.
        const { assert!(WARMUP_MAX_FRAMES >= 30 * 15) };
        const { assert!(WARMUP_MAX_FRAMES > STUCK_FRAMES + QUIET_FRAMES + 100) };
    }
}
