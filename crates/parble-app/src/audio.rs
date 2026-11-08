//! The audio event stream (P4-T3): the sim's recorded events, coalesced once per render frame
//! into an [`AudioFrame`] the bridge sends to the Web Audio engine.
//!
//! Per frame and per marble:
//! - contacts: every contact start in the frame folds into one [`AudioContact`] carrying the
//!   largest impulse and that contact's surface ([`AudioContact::fold`]), plus the count;
//! - rolls: the sim's 60 Hz [`RollSample`]s fold into one [`AudioRoll`] with the latest speed, the
//!   latest touching surface, and whether any sample touched ([`AudioRoll::fold`]). A frame with no
//!   sample tick (1–3 ticks at 120 Hz and up) has no rolls; the engine holds the last one.
//!
//! Positions are the marble centres at the frame's last tick. Contacts and rolls carry the marble's
//! size and mechanism events their piece id, so the engine can band impacts by real mass and pick a
//! mechanism's cue. The stream is read-only telemetry: recording and draining never change a run.

use parble_sim::Sim;
use parble_sim::marble::{MarbleKind, MarbleSize};
use parble_sim::mechanism::{
    ContactOther, InstanceId, MechEventKind, MechanismEvent, RollSample, SimEvent, SurfaceId,
};
use serde::{Deserialize, Serialize};

/// Marble type on the audio wire: [`MarbleKind::name`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WireMarble {
    #[default]
    CatsEye,
    Glazed,
    Steel,
    Swirl,
    Onionskin,
    Opal,
    Glow,
}

impl From<MarbleKind> for WireMarble {
    fn from(k: MarbleKind) -> Self {
        match k {
            MarbleKind::CatsEye => Self::CatsEye,
            MarbleKind::Glazed => Self::Glazed,
            MarbleKind::Steel => Self::Steel,
            MarbleKind::Swirl => Self::Swirl,
            MarbleKind::Onionskin => Self::Onionskin,
            MarbleKind::Opal => Self::Opal,
            MarbleKind::Glow => Self::Glow,
        }
    }
}

/// Marble size on the audio wire: [`MarbleSize::name`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WireSize {
    Mini,
    #[default]
    Standard,
    Shooter,
}

impl From<MarbleSize> for WireSize {
    fn from(s: MarbleSize) -> Self {
        match s {
            MarbleSize::Mini => Self::Mini,
            MarbleSize::Standard => Self::Standard,
            MarbleSize::Shooter => Self::Shooter,
        }
    }
}

/// The size whose radius is nearest `radius_m` (a marble's collider radius).
pub fn size_of_radius(radius_m: f32) -> MarbleSize {
    MarbleSize::ALL
        .into_iter()
        .min_by(|a, b| {
            (a.radius_m() - radius_m)
                .abs()
                .total_cmp(&(b.radius_m() - radius_m).abs())
        })
        .unwrap_or_default()
}

/// Contact surface on the audio wire: a kit surface by its sidecar name, or `marble` for another
/// marble.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WireSurface {
    Wood,
    Rail,
    WoodEndGrain,
    Marble,
}

impl From<SurfaceId> for WireSurface {
    fn from(s: SurfaceId) -> Self {
        use parble_kit::Surface;
        match s {
            SurfaceId::Glass => Self::Marble,
            SurfaceId::Kit(Surface::Wood) => Self::Wood,
            SurfaceId::Kit(Surface::Rail) => Self::Rail,
            SurfaceId::Kit(Surface::WoodEndGrain) => Self::WoodEndGrain,
        }
    }
}

/// One marble's contact starts in a frame, coalesced.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AudioContact {
    pub marble: u32,
    pub kind: WireMarble,
    pub size: WireSize,
    /// The strongest contact's surface.
    pub surface: WireSurface,
    /// The other marble's type when `surface` is `marble`.
    pub other_kind: Option<WireMarble>,
    /// The largest normal impulse among the coalesced contacts, N·s.
    pub impulse: f32,
    /// Contact starts coalesced into this one.
    pub count: u32,
    /// Marble centre, metres (the sim and render share one world frame).
    pub position: [f32; 3],
}

/// One marble's roll state in a frame.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AudioRoll {
    pub marble: u32,
    pub kind: WireMarble,
    pub size: WireSize,
    /// The latest sample's speed, m/s.
    pub speed: f32,
    /// The latest touching sample's carrying surface; `null` when no sample touched.
    pub surface: Option<WireSurface>,
    /// Any sample in the frame touched something.
    pub contact: bool,
    pub position: [f32; 3],
    /// The marble is inside a redirect's bored tunnel ([`in_redirect_tunnel`]) at the frame's
    /// last tick: the engine layers the tunnel rumble over its roll.
    pub tunnel: bool,
}

/// A mechanism event with its instance's anchor cell and piece id (empty when the instance is
/// gone by the time the frame is coalesced).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum AudioMechanism {
    SensorEnter {
        tick: u64,
        instance: [i32; 3],
        piece: String,
        sensor: u32,
        marble: u32,
    },
    SensorExit {
        tick: u64,
        instance: [i32; 3],
        piece: String,
        sensor: u32,
        marble: u32,
    },
    Signal {
        tick: u64,
        instance: [i32; 3],
        piece: String,
        name: String,
        value: i64,
    },
}

/// The main camera, for the Web Audio listener: world position and unit forward and up vectors.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AudioCamera {
    pub position: [f32; 3],
    pub forward: [f32; 3],
    pub up: [f32; 3],
}

/// The audio telemetry for one frame (or, after [`AudioFrame::absorb_older`], every frame since
/// the last drain).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AudioFrame {
    /// Sim tick at the end of the frame.
    pub tick: u64,
    /// At most one per marble, by marble index.
    pub contacts: Vec<AudioContact>,
    /// At most one per marble, by marble index.
    pub rolls: Vec<AudioRoll>,
    /// In tick order; at most [`MAX_AUDIO_MECHANISMS`], the newest.
    pub mechanisms: Vec<AudioMechanism>,
    /// `null` when there is no main camera.
    pub camera: Option<AudioCamera>,
    /// Playback is paused ([`crate::playback`]): the engine releases its roll loops. Sent only
    /// when true. A single step's contacts and mechanism events still play.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub paused: bool,
}

/// Mechanism events kept across undrained frames: the native build never drains.
pub const MAX_AUDIO_MECHANISMS: usize = 64;

impl AudioContact {
    /// Fold in `next`, a later contact of the same marble: the larger impulse keeps its surface,
    /// other marble and position (a tie keeps the earlier), and the counts add.
    pub fn fold(&mut self, next: AudioContact) {
        let count = self.count + next.count;
        if next.impulse > self.impulse {
            *self = next;
        }
        self.count = count;
    }
}

impl AudioRoll {
    /// Fold in `next`, a later roll of the same marble: its speed and position, its surface when it
    /// touched (else the earlier one's), and contact when either touched.
    pub fn fold(&mut self, next: AudioRoll) {
        let contact = self.contact || next.contact;
        let surface = next.surface.or(self.surface);
        *self = next;
        self.contact = contact;
        self.surface = surface;
    }
}

impl AudioFrame {
    /// Merge an undrained earlier frame into this one, so nothing is lost when the page drains less
    /// often than the app renders: contacts and rolls fold per marble (`older` first), mechanism
    /// events concatenate and keep the newest [`MAX_AUDIO_MECHANISMS`], and the tick and camera are
    /// this frame's (`older`'s camera when this one has none).
    pub fn absorb_older(&mut self, older: AudioFrame) {
        let contacts = core::mem::take(&mut self.contacts);
        self.contacts = fold_by_marble(older.contacts, contacts, |c| c.marble, AudioContact::fold);
        let rolls = core::mem::take(&mut self.rolls);
        self.rolls = fold_by_marble(older.rolls, rolls, |r| r.marble, AudioRoll::fold);
        let mut mechanisms = older.mechanisms;
        mechanisms.append(&mut self.mechanisms);
        mechanisms.drain(..mechanisms.len().saturating_sub(MAX_AUDIO_MECHANISMS));
        self.mechanisms = mechanisms;
        if self.camera.is_none() {
            self.camera = older.camera;
        }
    }
}

/// `acc` with each of `later` folded into its marble's entry (or appended), sorted by marble.
fn fold_by_marble<T: Copy>(
    mut acc: Vec<T>,
    later: impl IntoIterator<Item = T>,
    marble: impl Fn(&T) -> u32,
    fold: impl Fn(&mut T, T),
) -> Vec<T> {
    for n in later {
        match acc.iter_mut().find(|o| marble(o) == marble(&n)) {
            Some(o) => fold(o, n),
            None => acc.push(n),
        }
    }
    acc.sort_by_key(|t| marble(t));
    acc
}

/// One marble as the coalescer sees it: type, size and centre at the frame's last tick, and
/// whether that centre is in a redirect tunnel.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct MarbleAt {
    pub kind: MarbleKind,
    pub size: MarbleSize,
    pub position: [f32; 3],
    pub tunnel: bool,
}

/// `sim`'s marbles in spawn order: the type from `kinds` (a cat's eye past its end), the size from
/// the collider radius.
pub fn marbles_at(sim: &Sim, kinds: &[MarbleKind]) -> Vec<MarbleAt> {
    sim.marbles()
        .iter()
        .zip(sim.marble_radii())
        .enumerate()
        .map(|(i, (m, r))| MarbleAt {
            kind: kinds.get(i).copied().unwrap_or_default(),
            size: size_of_radius(r),
            position: m.position,
            tunnel: in_redirect_tunnel(sim, m.position),
        })
        .collect()
}

/// The redirect drop pieces: 1 × 1 × 2, the marble enters the top cell, drops through a hole and
/// rolls out of the lower cell through a tunnel bored in the block (kit contract §9 rule 7).
pub const REDIRECT_PIECES: [&str; 2] = ["blk_redirect_l2", "blk_redirect_l3"];

/// The cell holding the world point `p`: cells are centred on `x · cell` and `z · cell` and span
/// `y · cell … (y + 1) · cell` vertically.
pub fn cell_of(p: [f32; 3]) -> parble_core::CellCoord {
    let c = parble_sim::scene::cell_m();
    parble_core::CellCoord::new(
        (p[0] / c + 0.5).floor() as i32,
        (p[1] / c).floor() as i32,
        (p[2] / c + 0.5).floor() as i32,
    )
}

/// How far below a marble's centre [`in_redirect_tunnel`] probes. A marble rolling out of the
/// tunnel at L3 has its centre on the top face of the lower cell (allpieces' second redirect sits
/// at y = 0.3001 m), so the centre alone flips between cells; the probe point is inside the bore
/// for every marble size.
pub const TUNNEL_PROBE_DROP_M: f32 = 0.004;

/// Whether the marble centred at `p` is in the lower cell of a redirect placement, the cell its
/// tunnel runs through. The footprint's lower cell is the anchor's (0, −1, 0) at every yaw and
/// mirror.
pub fn in_redirect_tunnel(sim: &Sim, p: [f32; 3]) -> bool {
    let cell = tunnel_probe_cell(p);
    sim.chunk_colliders()
        .placement_at(cell)
        .is_some_and(|pl| REDIRECT_PIECES.contains(&pl.piece.as_str()) && cell.y == pl.anchor.y - 1)
}

/// The cell [`in_redirect_tunnel`] tests: the one holding the point [`TUNNEL_PROBE_DROP_M`] below `p`.
pub fn tunnel_probe_cell(p: [f32; 3]) -> parble_core::CellCoord {
    cell_of([p[0], p[1] - TUNNEL_PROBE_DROP_M, p[2]])
}

fn index(i: usize) -> u32 {
    u32::try_from(i).unwrap_or(u32::MAX)
}

/// Coalesce one frame's drained `events` (in sim order) into an [`AudioFrame`] at `tick`, with no
/// camera. `marbles` is indexed by sim marble index; a marble missing from it reads as a standard
/// cat's eye at the origin. `piece` names a mechanism instance's piece ([`Sim::mechanism_piece`]).
pub fn coalesce(
    events: &[SimEvent],
    marbles: &[MarbleAt],
    piece: impl Fn(InstanceId) -> Option<String>,
    tick: u64,
) -> AudioFrame {
    let at = |i: usize| marbles.get(i).copied().unwrap_or_default();
    let mut contacts = Vec::new();
    let mut rolls = Vec::new();
    let mut mechanisms = Vec::new();
    for e in events {
        match e {
            SimEvent::Contact(c) => {
                let m = at(c.marble);
                let other_kind = match c.other {
                    ContactOther::Marble(j) => Some(at(j).kind.into()),
                    ContactOther::Static | ContactOther::Part { .. } => None,
                };
                contacts.push(AudioContact {
                    marble: index(c.marble),
                    kind: m.kind.into(),
                    size: m.size.into(),
                    surface: c.surface.into(),
                    other_kind,
                    impulse: c.impulse,
                    count: 1,
                    position: m.position,
                });
            }
            SimEvent::Roll(RollSample {
                marble,
                speed,
                surface,
                ..
            }) => {
                let m = at(*marble);
                rolls.push(AudioRoll {
                    marble: index(*marble),
                    kind: m.kind.into(),
                    size: m.size.into(),
                    speed: *speed,
                    surface: surface.map(WireSurface::from),
                    contact: surface.is_some(),
                    position: m.position,
                    tunnel: m.tunnel,
                });
            }
            SimEvent::Mechanism(m) => {
                mechanisms.push(mechanism(m, piece(m.instance).unwrap_or_default()));
            }
        }
    }
    mechanisms.drain(..mechanisms.len().saturating_sub(MAX_AUDIO_MECHANISMS));
    AudioFrame {
        tick,
        contacts: fold_by_marble(Vec::new(), contacts, |c| c.marble, AudioContact::fold),
        rolls: fold_by_marble(Vec::new(), rolls, |r| r.marble, AudioRoll::fold),
        mechanisms,
        camera: None,
        paused: false,
    }
}

/// Holds a slowed run's contacts, rolls and mechanism events until at least `min_ticks` sim ticks
/// have passed since the last flush, so slow motion (one tick or none per frame) sends no more
/// impact one-shots per sim second than 1x at 60 fps does. Every frame still goes out with its
/// tick, camera and pause state.
#[derive(Debug, Default)]
pub struct AudioPacer {
    pending: Option<AudioFrame>,
    flushed: u64,
}

impl AudioPacer {
    /// The frame to send for `frame`. `min_ticks` 0 (1x) sends everything at once.
    pub fn pace(&mut self, mut frame: AudioFrame, min_ticks: u64) -> AudioFrame {
        // A run restart (tick back to 0) drops the old run's held events and flushes at once.
        if let Some(older) = self.pending.take()
            && frame.tick >= self.flushed
        {
            frame.absorb_older(older);
        }
        if frame.tick >= self.flushed && frame.tick - self.flushed < min_ticks {
            let held = AudioFrame {
                tick: frame.tick,
                camera: frame.camera,
                paused: frame.paused,
                ..AudioFrame::default()
            };
            self.pending = Some(frame);
            return held;
        }
        self.flushed = frame.tick;
        frame
    }
}

fn mechanism(m: &MechanismEvent, piece: String) -> AudioMechanism {
    let c = m.instance.0;
    let instance = [c.x, c.y, c.z];
    match &m.kind {
        MechEventKind::SensorEnter { sensor, marble } => AudioMechanism::SensorEnter {
            tick: m.tick,
            instance,
            piece,
            sensor: index(*sensor),
            marble: index(*marble),
        },
        MechEventKind::SensorExit { sensor, marble } => AudioMechanism::SensorExit {
            tick: m.tick,
            instance,
            piece,
            sensor: index(*sensor),
            marble: index(*marble),
        },
        MechEventKind::Signal { name, value } => AudioMechanism::Signal {
            tick: m.tick,
            instance,
            piece,
            name: (*name).to_owned(),
            value: *value,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use parble_core::CellCoord;
    use parble_sim::mechanism::{ContactEvent, InstanceId};
    use parble_sim::{Sim, playroom_run};

    const WOOD: SurfaceId = SurfaceId::Kit(parble_kit::Surface::Wood);
    const RAIL: SurfaceId = SurfaceId::Kit(parble_kit::Surface::Rail);

    fn contact(
        tick: u64,
        marble: usize,
        other: ContactOther,
        surface: SurfaceId,
        impulse: f32,
    ) -> SimEvent {
        SimEvent::Contact(ContactEvent {
            tick,
            marble,
            other,
            surface,
            impulse,
        })
    }

    fn roll(tick: u64, marble: usize, speed: f32, surface: Option<SurfaceId>) -> SimEvent {
        SimEvent::Roll(RollSample {
            tick,
            marble,
            speed,
            surface,
        })
    }

    fn no_piece(_: InstanceId) -> Option<String> {
        None
    }

    fn two_marbles() -> Vec<MarbleAt> {
        vec![
            MarbleAt {
                kind: MarbleKind::Steel,
                size: MarbleSize::Shooter,
                position: [0.5, 0.25, 0.0],
                tunnel: false,
            },
            MarbleAt {
                kind: MarbleKind::Glazed,
                size: MarbleSize::Mini,
                position: [1.0, 0.0, -0.5],
                tunnel: true,
            },
        ]
    }

    #[test]
    fn contacts_over_several_ticks_collapse_to_one_per_marble_with_the_strongest_surface() {
        let events = [
            contact(9, 0, ContactOther::Static, WOOD, 0.002),
            contact(9, 1, ContactOther::Static, WOOD, 0.004),
            contact(10, 0, ContactOther::Marble(1), SurfaceId::Glass, 0.009),
            contact(11, 0, ContactOther::Static, RAIL, 0.003),
            contact(12, 1, ContactOther::Static, RAIL, 0.001),
        ];
        let f = coalesce(&events, &two_marbles(), no_piece, 12);
        assert_eq!(f.tick, 12);
        assert_eq!(
            f.contacts,
            vec![
                AudioContact {
                    marble: 0,
                    kind: WireMarble::Steel,
                    size: WireSize::Shooter,
                    surface: WireSurface::Marble,
                    other_kind: Some(WireMarble::Glazed),
                    impulse: 0.009,
                    count: 3,
                    position: [0.5, 0.25, 0.0],
                },
                AudioContact {
                    marble: 1,
                    kind: WireMarble::Glazed,
                    size: WireSize::Mini,
                    surface: WireSurface::Wood,
                    other_kind: None,
                    impulse: 0.004,
                    count: 2,
                    position: [1.0, 0.0, -0.5],
                },
            ]
        );
    }

    #[test]
    fn an_equal_impulse_keeps_the_earlier_contact() {
        let events = [
            contact(1, 0, ContactOther::Static, RAIL, 0.5),
            contact(2, 0, ContactOther::Static, WOOD, 0.5),
        ];
        let f = coalesce(&events, &two_marbles(), no_piece, 2);
        assert_eq!(f.contacts[0].surface, WireSurface::Rail);
        assert_eq!(f.contacts[0].count, 2);
    }

    #[test]
    fn rolls_keep_the_latest_speed_and_touching_surface_and_any_contact() {
        let events = [
            roll(4, 0, 0.25, Some(RAIL)),
            roll(4, 1, 0.5, None),
            roll(8, 0, 0.75, None),
            roll(8, 1, 1.5, None),
            roll(12, 0, 1.25, None),
        ];
        let f = coalesce(&events, &two_marbles(), no_piece, 12);
        assert_eq!(
            f.rolls,
            vec![
                AudioRoll {
                    marble: 0,
                    kind: WireMarble::Steel,
                    size: WireSize::Shooter,
                    speed: 1.25,
                    surface: Some(WireSurface::Rail),
                    contact: true,
                    position: [0.5, 0.25, 0.0],
                    tunnel: false,
                },
                AudioRoll {
                    marble: 1,
                    kind: WireMarble::Glazed,
                    size: WireSize::Mini,
                    speed: 1.5,
                    surface: None,
                    contact: false,
                    position: [1.0, 0.0, -0.5],
                    tunnel: true,
                },
            ]
        );
    }

    #[test]
    fn mechanism_events_pass_through_in_order() {
        let instance = InstanceId(CellCoord { x: 1, y: -2, z: 3 });
        let events = [
            SimEvent::Mechanism(MechanismEvent {
                tick: 5,
                instance,
                kind: MechEventKind::SensorEnter {
                    sensor: 0,
                    marble: 1,
                },
            }),
            SimEvent::Mechanism(MechanismEvent {
                tick: 6,
                instance,
                kind: MechEventKind::Signal {
                    name: "swing",
                    value: 1,
                },
            }),
        ];
        let piece = |id: InstanceId| (id == instance).then(|| "mech_gate_oneway_l1".to_owned());
        let f = coalesce(&events, &two_marbles(), piece, 6);
        assert_eq!(
            f.mechanisms,
            vec![
                AudioMechanism::SensorEnter {
                    tick: 5,
                    instance: [1, -2, 3],
                    piece: "mech_gate_oneway_l1".into(),
                    sensor: 0,
                    marble: 1,
                },
                AudioMechanism::Signal {
                    tick: 6,
                    instance: [1, -2, 3],
                    piece: "mech_gate_oneway_l1".into(),
                    name: "swing".into(),
                    value: 1,
                },
            ]
        );
    }

    #[test]
    fn a_frame_with_no_events_is_empty() {
        let f = coalesce(&[], &two_marbles(), no_piece, 40);
        assert_eq!(
            f,
            AudioFrame {
                tick: 40,
                ..AudioFrame::default()
            }
        );
    }

    #[test]
    fn consecutive_frames_coalesce_independently() {
        let marbles = two_marbles();
        let a = coalesce(
            &[
                contact(1, 0, ContactOther::Static, WOOD, 0.75),
                roll(4, 0, 0.5, Some(WOOD)),
            ],
            &marbles,
            no_piece,
            4,
        );
        let b = coalesce(
            &[contact(5, 0, ContactOther::Static, RAIL, 0.25)],
            &marbles,
            no_piece,
            8,
        );
        assert_eq!(a.contacts.len(), 1);
        assert_eq!(b.contacts[0].impulse, 0.25);
        assert_eq!(b.contacts[0].surface, WireSurface::Rail);
        assert_eq!(b.contacts[0].count, 1);
        assert!(b.rolls.is_empty(), "frame b has no roll sample of its own");
    }

    #[test]
    fn absorbing_an_older_frame_folds_per_marble_and_keeps_the_newer_tick_and_camera() {
        let marbles = two_marbles();
        let camera = AudioCamera {
            position: [0.0, 1.0, 2.0],
            forward: [0.0, 0.0, -1.0],
            up: [0.0, 1.0, 0.0],
        };
        let mut older = coalesce(
            &[
                contact(1, 0, ContactOther::Static, WOOD, 0.75),
                contact(1, 1, ContactOther::Static, WOOD, 0.125),
                roll(4, 0, 0.5, Some(WOOD)),
            ],
            &marbles,
            no_piece,
            4,
        );
        older.camera = Some(camera);
        let mut newer = coalesce(
            &[
                contact(5, 0, ContactOther::Static, RAIL, 0.25),
                roll(8, 0, 1.0, None),
                roll(8, 1, 2.0, None),
            ],
            &marbles,
            no_piece,
            8,
        );
        newer.absorb_older(older);
        assert_eq!(newer.tick, 8);
        assert_eq!(newer.camera, Some(camera));
        assert_eq!(newer.contacts.len(), 2);
        assert_eq!(
            (newer.contacts[0].impulse, newer.contacts[0].count),
            (0.75, 2)
        );
        assert_eq!(newer.contacts[0].surface, WireSurface::Wood);
        assert_eq!(newer.contacts[1].marble, 1);
        assert_eq!(newer.rolls[0].speed, 1.0);
        assert_eq!(newer.rolls[0].surface, Some(WireSurface::Wood));
        assert!(newer.rolls[0].contact);
        assert_eq!(newer.rolls[1].speed, 2.0);
    }

    #[test]
    fn absorbing_keeps_only_the_newest_mechanism_events() {
        let ev = |tick: u64| AudioMechanism::Signal {
            tick,
            instance: [0, 0, 0],
            piece: String::new(),
            name: "swing".into(),
            value: 0,
        };
        let mut f = AudioFrame {
            mechanisms: (100..110).map(ev).collect(),
            ..AudioFrame::default()
        };
        f.absorb_older(AudioFrame {
            mechanisms: (0..100).map(ev).collect(),
            ..AudioFrame::default()
        });
        assert_eq!(f.mechanisms.len(), MAX_AUDIO_MECHANISMS);
        assert_eq!(
            f.mechanisms.first(),
            Some(&ev(110 - MAX_AUDIO_MECHANISMS as u64))
        );
        assert_eq!(f.mechanisms.last(), Some(&ev(109)));
    }

    #[test]
    fn a_slowed_run_sends_its_events_once_per_four_ticks() {
        let marbles = two_marbles();
        let hit = |t: u64| {
            coalesce(
                &[contact(t, 0, ContactOther::Static, WOOD, 0.25)],
                &marbles,
                no_piece,
                t,
            )
        };
        let mut pacer = AudioPacer::default();
        // 1x sends every frame's events at once.
        assert_eq!(pacer.pace(hit(4), 0).contacts.len(), 1);
        // One tick per frame (0.25x): three frames held, the fourth carries all four contacts.
        for t in 5..8 {
            let f = pacer.pace(hit(t), 4);
            assert!(f.contacts.is_empty(), "tick {t}");
            assert_eq!(f.tick, t);
        }
        let f = pacer.pace(hit(8), 4);
        assert_eq!((f.tick, f.contacts.len(), f.contacts[0].count), (8, 1, 4));
        // Paused frames hold the tick and keep the pause flag; a restart flushes at once.
        let paused = pacer.pace(
            AudioFrame {
                tick: 8,
                paused: true,
                ..AudioFrame::default()
            },
            4,
        );
        assert!(paused.paused);
        assert_eq!(pacer.pace(hit(0), 4).contacts.len(), 1);
    }

    #[test]
    fn a_restart_while_slowed_sends_none_of_the_old_runs_events() {
        let marbles = two_marbles();
        let mut pacer = AudioPacer::default();
        assert_eq!(
            pacer.pace(coalesce(&[], &marbles, no_piece, 100), 4).tick,
            100
        );
        // Held: fewer than 4 ticks since the last flush.
        let old = coalesce(
            &[
                contact(101, 0, ContactOther::Static, RAIL, 0.75),
                roll(101, 1, 0.5, Some(RAIL)),
            ],
            &marbles,
            no_piece,
            101,
        );
        assert!(pacer.pace(old, 4).contacts.is_empty());
        // The run restarts: only the new run's contact goes out.
        let new = coalesce(
            &[contact(0, 0, ContactOther::Static, WOOD, 0.25)],
            &marbles,
            no_piece,
            0,
        );
        let f = pacer.pace(new, 4);
        assert_eq!(f.tick, 0);
        assert_eq!(f.contacts.len(), 1);
        assert_eq!((f.contacts[0].impulse, f.contacts[0].count), (0.25, 1));
        assert_eq!(f.contacts[0].surface, WireSurface::Wood);
        assert!(f.rolls.is_empty());
    }

    #[test]
    fn wire_marble_names_match_the_sim_names() {
        for k in MarbleKind::ALL {
            let wire = serde_json::to_value(WireMarble::from(k)).unwrap();
            assert_eq!(wire, k.name(), "{k:?}");
        }
    }

    fn playroom_marbles(sim: &Sim) -> Vec<MarbleAt> {
        sim.marbles()
            .iter()
            .zip(playroom_run::MARBLES)
            .map(|(m, (kind, size))| MarbleAt {
                kind,
                size,
                position: m.position,
                tunnel: false,
            })
            .collect()
    }

    /// The real playroom train at 1–12 ticks per frame: every contact start lands in exactly one
    /// frame's coalesced counts, rolls appear only in frames that contain a sample tick, and the
    /// run's hash is the committed golden.
    #[test]
    fn the_recorded_playroom_coalesces_per_frame_with_nothing_lost_or_leaked() {
        let mut sim = Sim::playroom_scene();
        sim.set_event_recording(true);
        let per_frame = [1u32, 4, 12, 3, 2, 5, 4, 7, 1, 12];
        let (mut raw_contacts, mut coalesced_contacts, mut marble_contacts) = (0u32, 0u32, 0);
        let mut merged = AudioFrame::default();
        let mut i = 0;
        while sim.tick_count() < u64::from(playroom_run::PLAYROOM_TICKS) {
            let n = per_frame[i % per_frame.len()]
                .min(playroom_run::PLAYROOM_TICKS - sim.tick_count() as u32);
            i += 1;
            let start = sim.tick_count();
            sim.tick(n);
            let events = sim.drain_events();
            let f = coalesce(&events, &playroom_marbles(&sim), no_piece, sim.tick_count());
            let raw: Vec<_> = events
                .iter()
                .filter_map(|e| match e {
                    SimEvent::Contact(c) => Some(c),
                    _ => None,
                })
                .collect();
            assert!(
                raw.iter()
                    .all(|c| c.tick > start && c.tick <= sim.tick_count())
            );
            raw_contacts += raw.len() as u32;
            coalesced_contacts += f.contacts.iter().map(|c| c.count).sum::<u32>();
            marble_contacts += f.contacts.len();
            let mut marbles: Vec<_> = raw.iter().map(|c| index(c.marble)).collect();
            marbles.sort_unstable();
            marbles.dedup();
            assert_eq!(
                f.contacts.iter().map(|c| c.marble).collect::<Vec<_>>(),
                marbles
            );
            for c in &f.contacts {
                let max = raw
                    .iter()
                    .filter(|r| index(r.marble) == c.marble)
                    .map(|r| r.impulse)
                    .fold(f32::MIN, f32::max);
                assert_eq!(c.impulse, max);
            }
            let sample_tick = (start + 1..=sim.tick_count()).any(|t| t % 4 == 0);
            assert_eq!(
                f.rolls.len(),
                if sample_tick { sim.marbles().len() } else { 0 },
                "frame ({start}, {}]",
                sim.tick_count()
            );
            let mut next = f;
            next.absorb_older(core::mem::take(&mut merged));
            merged = next;
        }
        assert!(raw_contacts > 0, "the playroom train makes contacts");
        assert!(marble_contacts <= raw_contacts as usize);
        assert_eq!(coalesced_contacts, raw_contacts);
        // A page that never drained: the outbox folds every frame into one, still one contact per
        // marble carrying every start.
        assert!(merged.contacts.len() <= sim.marbles().len());
        assert!(merged.contacts.len() < raw_contacts as usize);
        assert_eq!(
            merged.contacts.iter().map(|c| c.count).sum::<u32>(),
            raw_contacts
        );
        assert_eq!(merged.tick, u64::from(playroom_run::PLAYROOM_TICKS));
        assert_eq!(sim.trajectory_hash(), playroom_run::playroom_4s_hash());
    }

    /// The live app records every run; the `.parble` fixtures must still reach their committed
    /// `runHash` with recording on and a drain every 4 ticks.
    #[test]
    fn recording_and_draining_leave_every_parble_fixture_hash_unchanged() {
        let all: serde_json::Value =
            serde_json::from_str(include_str!("../../../web/ui/src/parble-fixtures.json")).unwrap();
        let ticks = u32::try_from(all["runTicks"].as_u64().unwrap()).unwrap();
        for case in all["valid"].as_array().unwrap() {
            let f = parble_core::file::load(&case["file"].to_string()).unwrap();
            let mut run = parble_sim::track_run::TrackRun::new(&f.track).unwrap();
            run.set_event_recording(true);
            for _ in 0..ticks / 4 {
                run.tick(4);
                run.drain_events();
            }
            assert_eq!(
                parble_sim::hash::format_hash(run.trajectory_hash()),
                case["runHash"].as_str().unwrap(),
                "{}",
                case["name"]
            );
        }
    }

    #[test]
    fn each_size_radius_maps_back_to_its_size() {
        for s in MarbleSize::ALL {
            assert_eq!(size_of_radius(s.radius_m()), s);
            assert_eq!(serde_json::to_value(WireSize::from(s)).unwrap(), s.name());
        }
        assert_eq!(size_of_radius(0.0), MarbleSize::Mini);
        assert_eq!(size_of_radius(1.0), MarbleSize::Shooter);
    }

    /// A loaded track's hopper releases a mini, a standard and a shooter: the audio frames carry
    /// each marble's size, and the hopper's signals name its piece.
    #[test]
    fn a_loaded_track_reports_marble_sizes_and_mechanism_pieces() {
        use parble_core::{HopperConfig, PieceId, Placement, ReleaseMode, Yaw4};
        let mut track = parble_core::Track::new(3);
        track.placements = vec![Placement::new(
            PieceId::new(parble_sim::behaviour::HOPPER).unwrap(),
            CellCoord::new(6, 0, 0),
            Yaw4::new(0),
            false.into(),
        )];
        track.hopper = HopperConfig {
            placement: Some(0),
            queue: vec![
                parble_core::MarbleSize::Mini,
                parble_core::MarbleSize::Standard,
                parble_core::MarbleSize::Shooter,
            ],
            release: ReleaseMode::All,
        };
        let mut run = parble_sim::track_run::TrackRun::new(&track).unwrap();
        run.set_event_recording(true);
        let (mut sizes, mut signals) = (Vec::new(), Vec::new());
        for _ in 0..600 / 4 {
            run.tick(4);
            let events = run.drain_events();
            let marbles = marbles_at(&run, &[]);
            let piece = |id| run.mechanism_piece(id).map(|p| p.as_str().to_owned());
            let f = coalesce(&events, &marbles, piece, run.tick_count());
            for r in &f.rolls {
                if sizes.len() <= r.marble as usize {
                    sizes.resize(r.marble as usize + 1, None);
                }
                sizes[r.marble as usize] = Some(r.size);
            }
            for m in f.mechanisms {
                if let AudioMechanism::Signal { piece, name, .. } = m {
                    signals.push((piece, name));
                }
            }
        }
        assert_eq!(
            sizes,
            vec![
                Some(WireSize::Mini),
                Some(WireSize::Standard),
                Some(WireSize::Shooter)
            ]
        );
        let releases = signals.iter().filter(|(_, n)| n == "release").count();
        assert_eq!(releases, 3, "{signals:?}");
        assert!(
            signals
                .iter()
                .all(|(p, _)| p == parble_sim::behaviour::HOPPER),
            "{signals:?}"
        );
    }

    /// A marble the hopper releases rolls over an `L1` block and two `L1` rail straights, then off
    /// the rails' open end onto the floor: the coalesced audio frames report `wood` while it is over
    /// the block and `rail` over the rails, on the wire too.
    #[test]
    fn a_marble_on_rails_reports_rail_and_on_a_block_wood() {
        use parble_core::{HopperConfig, MarbleSize, PieceId, Placement, ReleaseMode, Yaw4};
        let place = |piece: &str, x: i32| {
            Placement::new(
                PieceId::new(piece).unwrap(),
                CellCoord::new(x, 0, 0),
                Yaw4::new(0),
                false.into(),
            )
        };
        let mut track = parble_core::Track::new(3);
        track.placements = vec![
            place("mech_hopper", 6),
            place("blk_straight_l1", 7),
            place("rail_straight_l1", 8),
            place("rail_straight_l1", 9),
        ];
        track.hopper = HopperConfig {
            placement: Some(0),
            queue: vec![MarbleSize::Standard],
            release: ReleaseMode::One,
        };
        let mut run = parble_sim::track_run::TrackRun::new(&track).unwrap();
        run.set_event_recording(true);
        // Cell x spans x ± 30 mm: the block is cell 7, the rails cells 8..9.
        let cell_x = |x: f32| ((x + 0.03) / 0.06).floor() as i32;
        let (mut rail, mut wood, mut max_x) = (0, 0, f32::MIN);
        let mut wire_rail = false;
        for _ in 0..2_400 / 4 {
            run.tick(4);
            let marbles: Vec<MarbleAt> = run
                .marbles()
                .iter()
                .map(|m| MarbleAt {
                    kind: MarbleKind::CatsEye,
                    size: parble_sim::marble::MarbleSize::Standard,
                    position: m.position,
                    tunnel: false,
                })
                .collect();
            let f = coalesce(&run.drain_events(), &marbles, no_piece, run.tick_count());
            for r in &f.rolls {
                let x = r.position[0];
                max_x = max_x.max(x);
                // Away from the seams, where the marble may touch both pieces, and on the track:
                // its centre rides 23 mm up on the block and the rails, 8 mm up on the floor.
                if (((x + 0.03) / 0.06).fract() - 0.5).abs() > 0.3
                    || !r.contact
                    || r.position[1] < 0.015
                {
                    continue;
                }
                match cell_x(x) {
                    7 => {
                        assert_eq!(r.surface, Some(WireSurface::Wood), "x {x:.4}");
                        wood += 1;
                    }
                    8 | 9 => {
                        assert_eq!(r.surface, Some(WireSurface::Rail), "x {x:.4}");
                        rail += 1;
                    }
                    _ => {}
                }
            }
            let json = serde_json::to_string(&f).unwrap();
            wire_rail |= json.contains(r#""surface":"rail""#);
        }
        assert!(
            max_x > 0.48,
            "the marble reaches the rails (max x {max_x:.3})"
        );
        assert!(rail > 5, "{rail} roll frames on the rails");
        assert!(wood > 5, "{wood} roll frames on the blocks");
        assert!(wire_rail, "an audioFrame carries surface \"rail\"");
    }

    /// A redirect's tunnel is its lower cell, at any yaw: the cell above it (the entry channel and
    /// the drop hole) and the neighbouring cells are not.
    #[test]
    fn the_redirect_tunnel_is_the_lower_cell_of_the_piece() {
        use parble_core::{PieceId, Placement, Yaw4};
        for (piece, yaw) in [
            ("blk_redirect_l3", 0),
            ("blk_redirect_l2", 1),
            ("blk_redirect_l3", 3),
        ] {
            let mut track = parble_core::Track::new(3);
            track.placements = vec![Placement::new(
                PieceId::new(piece).unwrap(),
                CellCoord::new(2, 2, -1),
                Yaw4::new(yaw),
                false.into(),
            )];
            let run = parble_sim::track_run::TrackRun::new(&track).unwrap();
            let c = parble_sim::scene::cell_m();
            let at = |x: f32, y: f32, z: f32| [x * c, y * c, z * c];
            let inside = |p| in_redirect_tunnel(&run, p);
            assert!(
                inside(at(2.0, 1.75, -1.0)),
                "{piece} yaw {yaw}: tunnel floor"
            );
            assert!(
                inside(at(2.4, 1.1, -0.6)),
                "{piece} yaw {yaw}: tunnel corner"
            );
            assert!(
                !inside(at(2.0, 2.75, -1.0)),
                "{piece} yaw {yaw}: entry cell"
            );
            assert!(!inside(at(3.0, 1.75, -1.0)), "{piece} yaw {yaw}: next cell");
            assert!(
                !inside(at(2.0, 0.75, -1.0)),
                "{piece} yaw {yaw}: cell below"
            );
            assert_eq!(cell_of(at(2.49, 1.99, -1.49)), CellCoord::new(2, 1, -1));
        }
    }

    /// The all-pieces track: every marble that passes a redirect rolls through its tunnel, and the
    /// audio frames flag those rolls `tunnel` (on the wire too), only while the marble is in a
    /// redirect's lower cell.
    #[test]
    fn rolls_through_the_all_pieces_redirects_are_flagged_tunnel() {
        use parble_sim::allpieces;
        let redirects: Vec<CellCoord> = allpieces::placements()
            .into_iter()
            .filter(|p| REDIRECT_PIECES.contains(&p.piece.as_str()))
            .map(|p| p.anchor + CellCoord::new(0, -1, 0))
            .collect();
        assert_eq!(redirects.len(), 3);
        let mut run = allpieces::run().unwrap();
        run.set_event_recording(true);
        let mut seen = std::collections::BTreeSet::new();
        let (mut flagged, mut moving, mut wire) = (0, 0, false);
        while run.tick_count() < u64::from(allpieces::RUN_TICKS) && seen.len() < redirects.len() {
            run.tick(4);
            let marbles = marbles_at(&run, &[]);
            let f = coalesce(&run.drain_events(), &marbles, no_piece, run.tick_count());
            for r in &f.rolls {
                let cell = tunnel_probe_cell(r.position);
                assert_eq!(r.tunnel, redirects.contains(&cell), "{r:?}");
                if r.tunnel {
                    seen.insert(cell);
                    flagged += 1;
                    if r.speed > 0.05 {
                        moving += 1;
                    }
                }
            }
            wire |= serde_json::to_string(&f)
                .unwrap()
                .contains(r#""tunnel":true"#);
        }
        assert_eq!(seen.len(), redirects.len(), "tunnels entered: {seen:?}");
        assert!(
            moving > 0 && moving * 2 > flagged,
            "{moving} of {flagged} moving"
        );
        assert!(wire, "an audioFrame carries \"tunnel\":true");
    }
}
