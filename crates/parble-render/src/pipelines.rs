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
//! that marble is first released. READY waits for it.

use std::sync::{Arc, Mutex, PoisonError};

use bevy::{
    prelude::*,
    render::{
        Render, RenderApp, RenderSystems,
        render_resource::{CachedPipelineState, PipelineCache, PipelineDescriptor},
    },
    shader::ShaderDefVal,
};
use serde::Serialize;

use crate::kit::KitDiag;

/// Render frames with nothing created or pending, after the textures loaded, that end the warm-up.
/// A texture that finishes loading on frame N has its material prepared and specialized a frame or
/// two later, so this leaves a margin.
pub const QUIET_FRAMES: u64 = 10;

/// Pipeline cache counts as of the last render frame.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PipelineCounts {
    /// Render frames that have run.
    pub render_frame: u64,
    pub total: usize,
    pub ok: usize,
    /// Queued or still being created. On wasm creation is synchronous, so after the frame's
    /// `process_queue` these are waiting for a shader or a shader import to load.
    pub pending: usize,
    /// Failed for good (a shader that does not compile); a shader still loading is pending.
    pub failed: usize,
    /// The render frame that last created a pipeline.
    pub last_created_frame: u64,
    /// Pipelines created on the last render frame: `label shader-path [shader defs]` (the fragment
    /// shader when the pipeline has one).
    pub created: Vec<String>,
}

impl PipelineCounts {
    /// True when the cache has pipelines and none is waiting.
    pub fn idle(&self) -> bool {
        self.total > 0 && self.pending == 0
    }
}

/// Loading warm-up: done (and latched) once the textures have loaded and the pipeline cache has
/// been quiet for [`QUIET_FRAMES`]. A pipeline created later (a look the warm-up did not draw) is
/// still listed in [`PipelineCounts::created`] but does not undo it.
#[derive(Resource, Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Warmup {
    pub done: bool,
    /// The render frame the warm-up ended on.
    pub done_frame: Option<u64>,
    /// The first render frame of the current run with every texture loaded (or failed).
    #[serde(skip)]
    loaded_frame: Option<u64>,
}

impl Warmup {
    fn update(&mut self, counts: &PipelineCounts, textures_loading: bool) {
        if self.done {
            return;
        }
        if textures_loading {
            self.loaded_frame = None;
            return;
        }
        let loaded = *self.loaded_frame.get_or_insert(counts.render_frame);
        if counts.idle()
            && counts.render_frame >= counts.last_created_frame.max(loaded) + QUIET_FRAMES
        {
            self.done = true;
            self.done_frame = Some(counts.render_frame);
            info!(
                "warm-up done on render frame {}: {} pipelines",
                counts.render_frame, counts.total
            );
        }
    }
}

#[derive(Default)]
struct Shared {
    counts: PipelineCounts,
    /// `ok` per cache index on the previous frame, to spot new pipelines.
    was_ok: Vec<bool>,
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
        .any(|t| matches!(t.state, "loading" | "notLoaded"));
    warmup.update(&stats.counts(), loading);
}

fn describe(d: &PipelineDescriptor) -> (String, Vec<String>) {
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
    let defs = stage
        .1
        .iter()
        .filter_map(|d| match d {
            ShaderDefVal::Bool(k, true) => Some(k.to_string()),
            ShaderDefVal::Bool(_, false) => None,
            ShaderDefVal::Int(k, v) => Some(format!("{k}={v}")),
            ShaderDefVal::UInt(k, v) => Some(format!("{k}={v}")),
        })
        .collect();
    (
        format!("{} {path}", label.as_deref().unwrap_or("(unlabelled)")),
        defs,
    )
}

fn count_pipelines(cache: Res<PipelineCache>, stats: Res<PipelineStats>) {
    let mut shared = stats.0.lock().unwrap_or_else(PoisonError::into_inner);
    let shared = &mut *shared;
    let frame = shared.counts.render_frame + 1;
    let mut counts = PipelineCounts {
        render_frame: frame,
        last_created_frame: shared.counts.last_created_frame,
        ..default()
    };
    for (i, p) in cache.pipelines().enumerate() {
        counts.total += 1;
        let ok = matches!(p.state, CachedPipelineState::Ok(_));
        match p.state {
            CachedPipelineState::Ok(_) => counts.ok += 1,
            CachedPipelineState::Err(_) => counts.failed += 1,
            CachedPipelineState::Queued | CachedPipelineState::Creating(_) => counts.pending += 1,
        }
        if i >= shared.was_ok.len() {
            shared.was_ok.push(false);
        }
        if ok && !shared.was_ok[i] {
            let (name, defs) = describe(&p.descriptor);
            counts.created.push(format!("{name} [{}]", defs.join(" ")));
        }
        shared.was_ok[i] = ok;
    }
    if !counts.created.is_empty() {
        counts.last_created_frame = frame;
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

    #[test]
    fn warmup_waits_for_quiet_frames_after_the_last_creation_and_latches() {
        let mut w = Warmup::default();
        w.update(&counts(5, 0, 5), false);
        assert!(!w.done);
        w.update(&counts(5 + QUIET_FRAMES - 1, 0, 5), false);
        assert!(!w.done);
        w.update(&counts(5 + QUIET_FRAMES, 1, 5), false);
        assert!(!w.done, "a pending pipeline holds it");
        w.update(&counts(5 + QUIET_FRAMES, 0, 5), false);
        assert!(w.done);
        assert_eq!(w.done_frame, Some(5 + QUIET_FRAMES));
        w.update(&counts(100, 3, 100), true);
        assert!(w.done, "latched");
    }

    #[test]
    fn warmup_counts_quiet_frames_from_when_the_textures_loaded() {
        let mut w = Warmup::default();
        w.update(&counts(30, 0, 2), true);
        assert!(!w.done);
        w.update(&counts(40, 0, 2), false);
        assert!(
            !w.done,
            "loaded on frame 40: their materials still have to specialize"
        );
        w.update(&counts(40 + QUIET_FRAMES, 0, 2), false);
        assert!(w.done);
    }

    #[test]
    fn an_empty_cache_is_not_idle() {
        let mut w = Warmup::default();
        let mut c = counts(50, 0, 0);
        c.total = 0;
        c.ok = 0;
        w.update(&c, false);
        assert!(!w.done);
    }
}
