//! Bus layout, routing, and side-chaining.
//!
//! The routing model: a main output bus plus optional auxiliary input
//! buses (side-chains, key inputs). Sources are routed into buses with
//! gain/mute; a [`SidechainDucker`] attenuates the main bus proportionally
//! to a key bus's short-term level (the gain-reduction core of a
//! side-chain compressor, deliberately kept level-following rather than
//! envelope-detected so it stays allocation-free and branch-simple).

use serde::{Deserialize, Serialize};
use tpt_av_audio_utils::{AudioBuffer, AudioError};

/// Identifier of a bus inside a [`BusLayout`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BusId(pub u32);

/// A routing from one source into a destination bus.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Route {
    /// Bus the source feeds.
    pub destination: BusId,
    /// Linear gain (1.0 = unity).
    pub gain: f32,
    /// Muted routes contribute silence.
    pub muted: bool,
}

/// Static bus arrangement for a plugin or mixer section.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BusLayout {
    /// Main output bus.
    pub main_output: BusId,
    /// Additional input buses (side-chain / key inputs), in stable order.
    pub sidechain_inputs: Vec<BusId>,
}

impl BusLayout {
    /// A main stereo output with no side-chains.
    pub fn stereo_main() -> Self {
        Self {
            main_output: BusId(0),
            sidechain_inputs: Vec::new(),
        }
    }

    /// A main output plus one named side-chain input.
    pub fn with_sidechain(id: u32) -> Self {
        Self {
            main_output: BusId(0),
            sidechain_inputs: vec![BusId(id)],
        }
    }
}

/// Routes source buffers into the main bus, with per-route gain/mute.
#[derive(Default)]
pub struct BusRouter {
    routes: Vec<Route>,
}

impl BusRouter {
    /// Creates an empty router.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a route.
    pub fn add_route(&mut self, route: Route) {
        self.routes.push(route);
    }

    /// Removes all routes to `destination`. Returns how many were removed.
    pub fn remove_routes_to(&mut self, destination: BusId) -> usize {
        let before = self.routes.len();
        self.routes.retain(|r| r.destination != destination);
        before - self.routes.len()
    }

    /// Current routes.
    pub fn routes(&self) -> &[Route] {
        &self.routes
    }

    /// Mixes `source` into `main` through every route that targets
    /// `source_bus`... — simplified: the router treats `sources[i]` as
    /// feeding `routes[i]`'s destination bus.
    ///
    /// # Real-Time Safety
    ///
    /// Allocation-free: all buffers are caller-owned.
    pub fn route_into_main(
        &self,
        sources: &[AudioBuffer],
        main: &mut AudioBuffer,
    ) -> Result<(), AudioError> {
        if sources.len() != self.routes.len() {
            return Err(AudioError::InvalidConfig(format!(
                "{} sources but {} routes",
                sources.len(),
                self.routes.len()
            )));
        }
        for (source, route) in sources.iter().zip(&self.routes) {
            if route.muted {
                continue;
            }
            if source.channels != main.channels {
                return Err(AudioError::InvalidConfig(format!(
                    "source channels {} != main bus channels {}",
                    source.channels, main.channels
                )));
            }
            main.mix_from(source, route.gain)?;
        }
        Ok(())
    }
}

/// Side-chain ducker: attenuates the main bus by the key bus's level.
///
/// Gain reduction per buffer: when the key's mean absolute level exceeds
/// `threshold`, the main bus is scaled by `1 - amount * (level/threshold - 1)`
/// capped at `1 - amount`. This is the classic "broadcast voice-over ducks
/// music" behavior in its simplest allocation-free form.
#[derive(Debug, Clone)]
pub struct SidechainDucker {
    /// Key level at/under which no ducking occurs (0 < threshold ≤ 1).
    pub threshold: f32,
    /// Maximum gain reduction (0.0 = no ducking, 1.0 = full mute).
    pub amount: f32,
    /// Smoothing factor for the ducking gain (0 = snap, 0.9 = heavy
    /// smoothing), applied per buffer.
    pub smoothing: f32,
    ducking_gain: f32,
}

impl SidechainDucker {
    /// Creates a ducker.
    pub fn new(threshold: f32, amount: f32) -> Self {
        Self {
            threshold: threshold.clamp(0.001, 1.0),
            amount: amount.clamp(0.0, 1.0),
            smoothing: 0.7,
            ducking_gain: 1.0,
        }
    }

    /// Computes the key signal's mean absolute level (mono fold-down).
    fn key_level(key: &AudioBuffer) -> f32 {
        if key.data.is_empty() {
            return 0.0;
        }
        let sum: f32 = key.data.iter().map(|s| s.abs()).sum();
        (sum / key.data.len() as f32).min(1.0)
    }

    /// Applies ducking to `main` based on `key`. Updates internal smoothing
    /// state; call once per buffer.
    pub fn process(&mut self, main: &mut AudioBuffer, key: &AudioBuffer) {
        let level = Self::key_level(key);
        let target = if level <= self.threshold {
            1.0
        } else {
            // Linear reduction above threshold.
            (1.0 - self.amount * (level / self.threshold - 1.0)).max(1.0 - self.amount)
        };
        self.ducking_gain += (target - self.ducking_gain) * (1.0 - self.smoothing);
        main.apply_gain(self.ducking_gain);
    }

    /// Current smoothed ducking gain (diagnostics).
    pub fn current_gain(&self) -> f32 {
        self.ducking_gain
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stereo(value: f32, frames: usize) -> AudioBuffer {
        let mut b = AudioBuffer::new(frames, 2);
        b.data.iter_mut().for_each(|s| *s = value);
        b
    }

    #[test]
    fn routes_sum_with_gains() {
        let mut router = BusRouter::new();
        router.add_route(Route {
            destination: BusId(0),
            gain: 1.0,
            muted: false,
        });
        router.add_route(Route {
            destination: BusId(0),
            gain: 0.5,
            muted: false,
        });

        let sources = [stereo(1.0, 4), stereo(1.0, 4)];
        let mut main = AudioBuffer::new(4, 2);
        router.route_into_main(&sources, &mut main).unwrap();
        assert!((main.data[0] - 1.5).abs() < 1e-6);
    }

    #[test]
    fn muted_route_is_silent() {
        let mut router = BusRouter::new();
        router.add_route(Route {
            destination: BusId(0),
            gain: 1.0,
            muted: true,
        });
        let sources = [stereo(1.0, 2)];
        let mut main = AudioBuffer::new(2, 2);
        router.route_into_main(&sources, &mut main).unwrap();
        assert!(main.data.iter().all(|&s| s == 0.0));
    }

    #[test]
    fn route_count_mismatch_is_error() {
        let mut router = BusRouter::new();
        router.add_route(Route {
            destination: BusId(0),
            gain: 1.0,
            muted: false,
        });
        let sources = [];
        let mut main = AudioBuffer::new(2, 2);
        assert!(router.route_into_main(&sources, &mut main).is_err());
    }

    #[test]
    fn remove_routes_by_destination() {
        let mut router = BusRouter::new();
        router.add_route(Route {
            destination: BusId(1),
            gain: 1.0,
            muted: false,
        });
        router.add_route(Route {
            destination: BusId(1),
            gain: 0.5,
            muted: false,
        });
        router.add_route(Route {
            destination: BusId(2),
            gain: 0.5,
            muted: false,
        });
        assert_eq!(router.remove_routes_to(BusId(1)), 2);
        assert_eq!(router.routes().len(), 1);
    }

    #[test]
    fn ducker_attenuates_loud_key() {
        let mut ducker = SidechainDucker::new(0.1, 0.6);
        ducker.smoothing = 0.0; // snap to target for a deterministic test
        let key = stereo(1.0, 128); // level 1.0 >> threshold 0.1
        let mut main = stereo(1.0, 128);
        ducker.process(&mut main, &key);

        // Target gain = 1 - 0.6 * (10 - 1), clamped at 1 - 0.6 = 0.4.
        assert!((ducker.current_gain() - 0.4).abs() < 1e-6);
        assert!((main.data[0] - 0.4).abs() < 1e-6);
    }

    #[test]
    fn quiet_key_leaves_main_untouched() {
        let mut ducker = SidechainDucker::new(0.5, 0.9);
        let key = stereo(0.01, 64);
        let mut main = stereo(1.0, 64);
        ducker.process(&mut main, &key);
        assert!((ducker.current_gain() - 1.0).abs() < 1e-6);
        assert!((main.data[0] - 1.0).abs() < 1e-6);
    }

    #[test]
    fn smoothing_moves_toward_target() {
        let mut ducker = SidechainDucker::new(0.1, 1.0);
        ducker.smoothing = 0.9;
        let key = stereo(1.0, 64);
        let mut main = stereo(1.0, 64);

        let mut first = None;
        for i in 0..5 {
            ducker.process(&mut main, &key);
            if i == 0 {
                first = Some(ducker.current_gain());
            }
        }
        // Gain keeps decreasing across buffers (smoothing in action).
        assert!(ducker.current_gain() < first.unwrap());
    }
}
