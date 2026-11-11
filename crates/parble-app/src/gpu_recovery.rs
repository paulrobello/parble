//! Recovery from a lost GPU device (WebGPU `device.lost`, a native driver reset).
//!
//! Bevy polls the device's lost callback once per frame and hands the loss to the
//! [`RenderErrorHandler`]. Its default quits the app; this handler instead returns
//! [`RenderErrorPolicy::Recover`], which requests a new adapter and device (asynchronously on wasm),
//! re-runs `RenderStartup` and re-extracts every render asset from the main world. Recovery is
//! render-only: the sim, the track, the editor and the marbles live in the main world and keep
//! running while the renderer is rebuilt.
//!
//! Some render state is not rebuilt by Bevy 0.20 and is reset here:
//! - the window's surface (`SurfaceData`, configured against the lost device, so the canvas would
//!   keep handing out the dead device's textures);
//! - the readiness tracking ([`parble_render::pipelines::restart`]), so READY waits for the rebuilt
//!   pipelines;
//! - assets in flight: `RenderStartup` re-extracts only the assets already prepared, and drops the
//!   ones still waiting to be prepared (a material waiting for its textures, an upload deferred by
//!   the bytes-per-frame limit), so a loss while loading would leave them undrawn for good;
//! - the LOD visibility ranges: `RenderStartup` empties `RenderVisibilityRanges`, which is extracted
//!   again only when a [`VisibilityRange`] changes.
//!
//! [`refresh_after_recovery`] handles the last two once the rebuilt renderer has run a frame, by
//! marking every mesh, image and material asset modified and every [`VisibilityRange`] changed, so
//! the next extraction sends them all again. Until then an error other than a loss comes from the
//! lost device (Safari reports validation errors from one) and only restarts the rebuild.
//!
//! At most [`MAX_LOSSES`] losses within [`WINDOW_S`] recover. One more ends the app on the fatal
//! path it took before recovery existed: rendering stops, the app exits, and on web
//! `window.__parble.fatal` names the loss. A device that keeps dying never loops.

use bevy::{
    camera::visibility::VisibilityRange,
    prelude::*,
    render::{
        error_handler::{ErrorType, RenderError, RenderErrorHandler, RenderErrorPolicy},
        settings::RenderCreation,
        view::window::SurfaceData,
    },
};

use parble_render::{
    kit::KitWoodMaterial,
    marble::{CoreGlassMaterial, VaneGlassMaterial},
    pipelines::PipelineStats,
    playroom::LanternMaterial,
};

use crate::bridge::{self, Event};

/// Losses that recover within [`WINDOW_S`]; the next one is fatal.
pub const MAX_LOSSES: usize = 3;
/// The window, in seconds of real time, [`MAX_LOSSES`] is counted over.
pub const WINDOW_S: f64 = 30.0;
/// Restarts of one rebuild after an error during it; one more is fatal.
pub const MAX_RESTARTS: u32 = 3;

/// Recoveries so far, and the recent losses the bound counts.
#[derive(Resource, Debug, Default)]
pub struct GpuRecovery {
    /// Lost devices the renderer was rebuilt after.
    pub recoveries: u32,
    /// Real time (s) of the losses within the last [`WINDOW_S`].
    losses: Vec<f64>,
    /// A rebuild is under way: the lost device is still the render world's until the new one has
    /// drawn a frame.
    rebuilding: bool,
    /// Restarts of the current rebuild ([`MAX_RESTARTS`]).
    restarts: u32,
}

impl GpuRecovery {
    /// Records a loss at `now` and answers whether it may recover.
    fn admit(&mut self, now: f64) -> bool {
        self.losses.retain(|t| now - t < WINDOW_S);
        self.losses.push(now);
        self.restarts = 0;
        self.losses.len() <= MAX_LOSSES
    }

    /// Records an error during a rebuild and answers whether the rebuild may start again.
    fn restart(&mut self) -> bool {
        self.restarts += 1;
        self.restarts <= MAX_RESTARTS
    }
}

pub struct GpuRecoveryPlugin;

impl Plugin for GpuRecoveryPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<GpuRecovery>()
            .insert_resource(RenderErrorHandler(handle))
            .add_systems(Update, refresh_after_recovery);
        page::claim();
    }
}

/// After each recovery, once the rebuilt renderer has drawn a frame (so `RenderStartup` has run),
/// marks every mesh, image and material asset modified and every [`VisibilityRange`] changed, so
/// the next extraction sends them all to the render world again.
#[allow(clippy::too_many_arguments)]
fn refresh_after_recovery(
    mut rec: ResMut<GpuRecovery>,
    stats: Res<PipelineStats>,
    mut refreshed: Local<u32>,
    mut ranges: Query<&mut VisibilityRange>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut standard: ResMut<Assets<StandardMaterial>>,
    mut wood: ResMut<Assets<KitWoodMaterial>>,
    mut lantern: ResMut<Assets<LanternMaterial>>,
    mut vane: ResMut<Assets<VaneGlassMaterial>>,
    mut core: ResMut<Assets<CoreGlassMaterial>>,
) {
    if rec.recoveries == *refreshed || stats.counts().render_frame == 0 {
        return;
    }
    *refreshed = rec.recoveries;
    rec.rebuilding = false;
    // `iter_mut` queues a `Modified` event per asset.
    meshes.iter_mut().for_each(drop);
    images.iter_mut().for_each(drop);
    standard.iter_mut().for_each(drop);
    wood.iter_mut().for_each(drop);
    lantern.iter_mut().for_each(drop);
    vane.iter_mut().for_each(drop);
    core.iter_mut().for_each(drop);
    for mut r in &mut ranges {
        r.set_changed();
    }
}

fn handle(error: &RenderError, main: &mut World, render: &mut World) -> RenderErrorPolicy {
    let lost = matches!(error.ty, ErrorType::DeviceLost);
    let mut rec = main.resource_mut::<GpuRecovery>();
    if !lost && rec.rebuilding {
        // Until the new device is in place Bevy still polls the lost one, and Safari reports
        // validation errors from a lost device. Such an error is the loss, not a new fault: ask for
        // the device again (the request in flight is dropped) without counting another loss. An
        // error that keeps coming back is a real one: past [`MAX_RESTARTS`] the app gives up.
        if rec.restart() {
            warn!(
                "{:?} error while rebuilding after a lost GPU device, restarting the rebuild: {}",
                error.ty, error.description
            );
            return RenderErrorPolicy::Recover(RenderCreation::default());
        }
        let message = format!(
            "{:?} error {} times while rebuilding after a lost GPU device, not recovering: {}",
            error.ty,
            MAX_RESTARTS + 1,
            error.description
        );
        return give_up(main, &message);
    }
    if !lost {
        // Validation and out-of-memory errors keep Bevy's default: the cause is still there, so
        // rendering on would repeat it.
        error!("Quitting the application due to {:?} RenderError", error.ty);
        main.write_message(AppExit::error());
        return RenderErrorPolicy::StopRendering;
    }
    let now = main.resource::<Time<Real>>().elapsed_secs_f64();
    let mut rec = main.resource_mut::<GpuRecovery>();
    if !rec.admit(now) {
        let message = format!(
            "GPU device lost more than {MAX_LOSSES} times in {WINDOW_S} s, not recovering: {}",
            error.description
        );
        return give_up(main, &message);
    }
    rec.recoveries += 1;
    rec.rebuilding = true;
    let count = rec.recoveries;
    warn!(
        "GPU device lost ({}): rebuilding the renderer, recovery {count}",
        error.description
    );
    let surfaces: Vec<Entity> = render
        .query_filtered::<Entity, With<SurfaceData>>()
        .iter(render)
        .collect();
    for entity in surfaces {
        render.entity_mut(entity).remove::<SurfaceData>();
    }
    parble_render::pipelines::restart(main);
    bridge::push_event(Event::GpuRecovered { count });
    page::recovering(count);
    // The same settings the app started with: `RenderPlugin`'s default.
    RenderErrorPolicy::Recover(RenderCreation::default())
}

/// Ends on the fatal path: rendering stops, the app exits, and on web the page records `message`.
fn give_up(main: &mut World, message: &str) -> RenderErrorPolicy {
    error!("{message}");
    page::gave_up(message);
    main.write_message(AppExit::error());
    RenderErrorPolicy::StopRendering
}

/// index.html's device-lost hook. It records every loss; unless the app has claimed recovery it
/// reports the loss as fatal at once, and when it has, it waits for the app's verdict.
#[cfg(target_arch = "wasm32")]
mod page {
    use js_sys::{Function, JsString, Reflect};
    use wasm_bindgen::{JsCast, JsValue};

    fn call(name: &str, args: &[JsValue]) {
        let Ok(f) = Reflect::get(&js_sys::global(), &JsString::from(name)) else {
            return;
        };
        let Ok(f) = f.dyn_into::<Function>() else {
            return;
        };
        let args: js_sys::Array = args.iter().collect();
        let _ = f.apply(&JsValue::NULL, &args);
    }

    pub fn claim() {
        let _ = Reflect::set(
            &js_sys::global(),
            &JsString::from("__parbleGpuRecovery"),
            &JsValue::TRUE,
        );
    }

    pub fn recovering(count: u32) {
        call("__parbleGpuRecovering", &[JsValue::from(count)]);
    }

    pub fn gave_up(message: &str) {
        call("__parbleGpuGaveUp", &[JsValue::from_str(message)]);
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod page {
    pub fn claim() {}
    pub fn recovering(_count: u32) {}
    pub fn gave_up(_message: &str) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn three_losses_in_the_window_recover_and_the_fourth_does_not() {
        let mut r = GpuRecovery::default();
        assert!(r.admit(1.0));
        assert!(r.admit(2.0));
        assert!(r.admit(3.0));
        assert!(!r.admit(4.0));
    }

    #[test]
    fn losses_older_than_the_window_do_not_count() {
        let mut r = GpuRecovery::default();
        assert!(r.admit(0.0));
        assert!(r.admit(10.0));
        assert!(r.admit(20.0));
        assert!(r.admit(31.0), "the loss at 0 s has left the window");
        assert!(!r.admit(32.0));
    }

    #[test]
    fn a_rebuild_restarts_at_most_three_times_and_a_new_loss_resets_the_count() {
        let mut r = GpuRecovery::default();
        assert!(r.admit(0.0));
        assert!(r.restart());
        assert!(r.restart());
        assert!(r.restart());
        assert!(!r.restart());
        assert!(r.admit(1.0));
        assert!(r.restart());
    }
}
