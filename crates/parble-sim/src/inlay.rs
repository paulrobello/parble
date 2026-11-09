//! Glow-inlay state: the on/off switch behind a lit inlay strip set into a mechanism (`ref-09`).
//!
//! A behaviour that lights an inlay owns one [`Inlay`] per strip group and saves it with
//! [`Inlay::save`] under `inlay/<i>/on` and `inlay/<i>/since`. The state is plain integers, so it
//! rides the framework's save/restore and the mechanism hash like any other behaviour state, and
//! it never feeds back into physics. The renderer reads it through [`inlays`] on a
//! [`crate::mechanism::MechanismView`] and turns `since` into a short fade
//! (`parble_render::inlay`).
//!
//! Reuse (stepper elevator, P7-T5 inlays): hold an `Inlay` per group, call [`Inlay::set`] from
//! `step` with the tick whenever the group should light or go dark, write it in `save` and read it
//! back with [`Inlay::restore`]. Indices are dense from 0.

use parble_core::MechConfig;

/// State keys of inlay `i` start with `inlay/<i>/`.
pub const INLAY_STATE_PREFIX: &str = "inlay/";

/// One inlay group: lit or dark, and the tick it last switched (`None`: never switched, so it
/// shows its state with no fade).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Inlay {
    pub on: bool,
    pub since: Option<u64>,
}

impl Inlay {
    /// Switch to `on` at `tick`. Setting the state it already has keeps its `since`.
    pub fn set(&mut self, on: bool, tick: u64) {
        if self.on != on {
            self.on = on;
            self.since = Some(tick);
        }
    }

    fn keys(i: usize) -> (String, String) {
        (
            format!("{INLAY_STATE_PREFIX}{i}/on"),
            format!("{INLAY_STATE_PREFIX}{i}/since"),
        )
    }

    /// Write inlay `i` into `state`.
    pub fn save(&self, i: usize, state: &mut MechConfig) {
        let (on, since) = Self::keys(i);
        state.insert(on, i64::from(self.on));
        state.insert(since, self.since.map_or(-1, |t| t as i64));
    }

    /// Inlay `i` from `state`, or `None` when `state` has no such inlay.
    pub fn restore(state: &MechConfig, i: usize) -> Option<Self> {
        let (on, since) = Self::keys(i);
        let on = *state.get(&on)?;
        let since = state.get(&since).copied().unwrap_or(-1);
        Some(Self {
            on: on != 0,
            since: u64::try_from(since).ok(),
        })
    }
}

/// Every inlay in `state`, in index order (stops at the first missing index).
pub fn inlays(state: &MechConfig) -> Vec<Inlay> {
    (0..).map_while(|i| Inlay::restore(state, i)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_keeps_since_on_a_repeat_and_round_trips() {
        let mut a = Inlay::default();
        a.set(true, 40);
        a.set(true, 90);
        assert_eq!(
            a,
            Inlay {
                on: true,
                since: Some(40)
            }
        );
        let mut s = MechConfig::new();
        a.save(0, &mut s);
        Inlay::default().save(1, &mut s);
        assert_eq!(inlays(&s), [a, Inlay::default()]);
        assert_eq!(Inlay::restore(&s, 2), None);
    }
}
