//! Virtual audio routing (future).
//!
//! A small in-process router: named sources render into the router's mix
//! buffer each cycle, scaled by per-route gain/mute. Today this backs tests
//! and the examples; when OS virtual-device support lands (e.g. a WASAPI
//! loopback repeater or PipeWire null-sink proxy), this type becomes the
//! mixing core behind it.

use std::collections::BTreeMap;

use tpt_av_audio_utils::{AudioBuffer, AudioError};

/// A source renderer: fills a buffer with audio each router cycle.
pub type SourceCallback = Box<dyn FnMut(&mut AudioBuffer) + Send>;

/// A route from a named source into the virtual mix bus.
#[derive(Debug, Clone)]
pub struct Route {
    /// Source name this route pulls from.
    pub source: String,
    /// Linear gain applied to the source (1.0 = unity).
    pub gain: f32,
    /// Whether the route is muted.
    pub muted: bool,
}

/// In-process mixing router.
///
/// Sources are callbacks (same real-time contract as stream callbacks); the
/// router owns one preallocated mix buffer sized by [`VirtualRouter::new`].
#[derive(Default)]
pub struct VirtualRouter {
    routes: BTreeMap<String, Route>,
    sources: BTreeMap<String, SourceCallback>,
}

impl VirtualRouter {
    /// Creates an empty router.
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers (or replaces) a source renderer under `name`.
    pub fn add_source(&mut self, name: &str, source: Box<dyn FnMut(&mut AudioBuffer) + Send>) {
        self.sources.insert(name.to_string(), source);
    }

    /// Adds (or updates) a route for `source`.
    pub fn add_route(&mut self, route: Route) {
        self.routes.insert(route.source.clone(), route);
    }

    /// Removes a route.
    pub fn remove_route(&mut self, source: &str) {
        self.routes.remove(source);
    }

    /// Current routes, sorted by source name.
    pub fn routes(&self) -> Vec<&Route> {
        self.routes.values().collect()
    }

    /// Renders one cycle: every routed source renders into its own scratch
    /// copy and is summed into `mix` with its gain. Sources without a route
    /// are silent; routes without a source contribute silence.
    ///
    /// Returns the number of sources actually mixed.
    pub fn render_once(
        &mut self,
        mix: &mut AudioBuffer,
        scratch: &mut AudioBuffer,
    ) -> Result<usize, AudioError> {
        if scratch.channels != mix.channels {
            return Err(AudioError::InvalidConfig(format!(
                "scratch channels {} != mix channels {}",
                scratch.channels, mix.channels
            )));
        }
        scratch.clear();
        mix.clear();

        let route_gains: Vec<(String, f32)> = self
            .routes
            .values()
            .filter(|r| !r.muted)
            .map(|r| (r.source.clone(), r.gain))
            .collect();

        let mut mixed = 0usize;
        for (name, gain) in &route_gains {
            if let Some(source) = self.sources.get_mut(name) {
                // Render into scratch, then add with gain. (One extra copy
                // versus a true n-bus mixer; the router is a host-side
                // utility, not the real-time graph.)
                source(scratch);
                mix.mix_from(scratch, *gain)?;
                scratch.clear();
                mixed += 1;
            }
        }
        Ok(mixed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(freq_frames: usize, value: f32) -> Box<dyn FnMut(&mut AudioBuffer) + Send> {
        Box::new(move |buf: &mut AudioBuffer| {
            for frame in 0..buf.frames {
                let v = if frame % freq_frames == 0 { value } else { 0.0 };
                for ch in 0..buf.channels as usize {
                    buf.data[frame * buf.channels as usize + ch] = v;
                }
            }
        })
    }

    #[test]
    fn mixes_sources_with_route_gain() {
        let mut router = VirtualRouter::new();
        router.add_source("a", tone(1, 1.0)); // every frame 1.0
        router.add_source("b", tone(1, 0.5)); // every frame 0.5
        router.add_route(Route {
            source: "a".into(),
            gain: 1.0,
            muted: false,
        });
        router.add_route(Route {
            source: "b".into(),
            gain: 0.5,
            muted: false,
        });

        let mut mix = AudioBuffer::new(8, 2);
        let mut scratch = AudioBuffer::new(8, 2);
        let mixed = router.render_once(&mut mix, &mut scratch).unwrap();

        assert_eq!(mixed, 2);
        assert!((mix.data[0] - 1.25).abs() < 1e-6); // 1.0*1.0 + 0.5*0.5
    }

    #[test]
    fn muted_routes_and_unrouted_sources_are_silent() {
        let mut router = VirtualRouter::new();
        router.add_source("a", tone(1, 1.0));
        router.add_source("quiet", tone(1, 0.9));
        router.add_route(Route {
            source: "a".into(),
            gain: 1.0,
            muted: true,
        });
        // "quiet" has no route: silent.

        let mut mix = AudioBuffer::new(4, 1);
        let mut scratch = AudioBuffer::new(4, 1);
        let mixed = router.render_once(&mut mix, &mut scratch).unwrap();
        assert_eq!(mixed, 0);
        assert!(mix.data.iter().all(|&s| s == 0.0));
    }

    #[test]
    fn remove_route_works() {
        let mut router = VirtualRouter::new();
        router.add_route(Route {
            source: "x".into(),
            gain: 1.0,
            muted: false,
        });
        router.remove_route("x");
        assert!(router.routes().is_empty());
    }

    #[test]
    fn channel_mismatch_is_invalid_config() {
        let mut router = VirtualRouter::new();
        let mut mix = AudioBuffer::new(4, 2);
        let mut scratch = AudioBuffer::new(4, 1);
        assert!(router.render_once(&mut mix, &mut scratch).is_err());
    }
}
