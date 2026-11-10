//! Whether the primary window got a swapchain texture on each render frame, so the agent can tell a
//! screenshot of a frame that was never presented from a real render failure.
//!
//! On macOS wgpu does not acquire a drawable while the window is occluded (covered by other
//! windows, on another Space, or the display asleep: wgpu-hal `metal::Surface::acquire_texture`),
//! and Bevy's `prepare_windows` drops that frame without a log. A [`Screenshot`] taken on such a
//! frame skips its texture-to-buffer copy but still maps the zero-initialised readback buffer, so it
//! arrives as an all-zero image. Tag the screenshot entity with a [`ShotAttempt`] and ask
//! [`PresentProbe::attempt_presented`] about its capture frame.
//!
//! [`Screenshot`]: bevy::render::view::screenshot::Screenshot

use std::collections::HashSet;
use std::sync::{Arc, Mutex, PoisonError};

use bevy::{
    prelude::*,
    render::{
        Extract, ExtractSchedule, Render, RenderApp, RenderSystems,
        view::{ExtractedWindow, prepare_windows},
    },
    window::{PrimaryWindow, WindowOccluded},
};

/// Tags a screenshot entity, so the render world can tell which render frame captured it.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ShotAttempt(pub u32);

/// Attempts remembered by [`PresentProbe`]; a run takes a handful.
const KEPT_ATTEMPTS: usize = 64;

#[derive(Default)]
struct Shared {
    render_frame: u64,
    /// The last render frame had a swapchain texture.
    presenting: bool,
    /// Render frames without one, since start.
    unpresented_frames: u64,
    /// Per attempt, whether its capture frame had a swapchain texture.
    attempts: Vec<(ShotAttempt, bool)>,
}

/// Shared between the main and render worlds.
#[derive(Resource, Clone, Default)]
pub struct PresentProbe(Arc<Mutex<Shared>>);

impl PresentProbe {
    fn lock(&self) -> std::sync::MutexGuard<'_, Shared> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Whether `attempt`'s capture frame had a swapchain texture; `None` if not rendered yet.
    pub fn attempt_presented(&self, attempt: ShotAttempt) -> Option<bool> {
        let s = self.lock();
        s.attempts
            .iter()
            .rev()
            .find(|(a, _)| *a == attempt)
            .map(|(_, p)| *p)
    }

    /// The last render frame had a swapchain texture.
    pub fn presenting(&self) -> bool {
        self.lock().presenting
    }

    /// `(render frame, render frames without a swapchain texture so far)`.
    pub fn frames(&self) -> (u64, u64) {
        let s = self.lock();
        (s.render_frame, s.unpresented_frames)
    }
}

/// Attempts first extracted on this render frame.
#[derive(Resource, Default)]
struct NewAttempts(Vec<ShotAttempt>);

pub struct PresentProbePlugin;

impl Plugin for PresentProbePlugin {
    fn build(&self, app: &mut App) {
        let probe = PresentProbe::default();
        app.insert_resource(probe.clone())
            .add_systems(PostUpdate, log_occluded);
        if let Some(render_app) = app.get_sub_app_mut(RenderApp) {
            render_app
                .insert_resource(probe)
                .init_resource::<NewAttempts>()
                .add_systems(ExtractSchedule, extract_attempts)
                .add_systems(
                    Render,
                    probe_present
                        .after(prepare_windows)
                        .in_set(RenderSystems::PrepareViews),
                );
        }
    }
}

fn log_occluded(mut occluded: MessageReader<WindowOccluded>, probe: Res<PresentProbe>) {
    for ev in occluded.read() {
        info!(
            "window {} occluded={} (render frame {})",
            ev.window,
            ev.occluded,
            probe.frames().0
        );
    }
}

fn extract_attempts(
    attempts: Extract<Query<&ShotAttempt>>,
    mut new: ResMut<NewAttempts>,
    mut seen: Local<HashSet<ShotAttempt>>,
) {
    new.0.clear();
    for a in &attempts {
        if seen.insert(*a) {
            new.0.push(*a);
        }
    }
}

fn probe_present(
    windows: Query<&ExtractedWindow, With<PrimaryWindow>>,
    new: Res<NewAttempts>,
    probe: Res<PresentProbe>,
) {
    let Ok(window) = windows.single() else {
        return;
    };
    let presented = window.swap_chain_texture_view.is_some();
    let mut s = probe.lock();
    s.render_frame += 1;
    let frame = s.render_frame;
    if presented != s.presenting {
        info!(
            "render frame {frame}: primary window {}",
            if presented {
                "presenting"
            } else {
                "not presented (no swapchain texture)"
            }
        );
    }
    s.presenting = presented;
    s.unpresented_frames += u64::from(!presented);
    for a in &new.0 {
        info!(
            "render frame {frame}: screenshot attempt {} captured, presented={presented}",
            a.0
        );
        s.attempts.push((*a, presented));
    }
    let over = s.attempts.len().saturating_sub(KEPT_ATTEMPTS);
    s.attempts.drain(..over);
}
