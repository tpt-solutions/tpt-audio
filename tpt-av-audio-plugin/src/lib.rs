//! # tpt-av-audio-plugin
//!
//! Plugin hosting foundation for the `tpt-av-audio-*` engine: parameter
//! automation wired into the timeline [`Envelope`](tpt_av_audio_timeline::Envelope)
//! model, plus bus routing and side-chaining.
//!
//! ## Hosting status
//!
//! **VST3 is not supported and will not be**: the Steinberg VST3 SDK is
//! GPLv3-or-proprietary dual-licensed, which conflicts with this
//! workspace's MIT/Apache-2.0-only `deny.toml` policy. Decision recorded
//! 2026-09-21.
//!
//! **CLAP** is supported behind the `clap` feature, via
//! [`clack-host`](https://docs.rs/clack-host) (pure Rust, `MIT OR
//! Apache-2.0`, crates.io) — see [`clap_host::ClapPluginNode`]. Note that
//! `nih-plug` — named in the original plan — is a plugin **development**
//! framework and deliberately does not expose a hosting API, so it could
//! not have provided this.
//!
//! Everything else in this crate is backend-agnostic and already useful:
//! hosts describe plugins with [`HostedPlugin`], automate parameters
//! through [`ParameterAutomation`] (envelope-driven, timeline-frame based),
//! and wire side-chains and buses with [`bus`].

// Hosting foundation: no unsafe anywhere, except the `clap` module, which
// is the sole, documented FFI boundary (see its module docs) for loading
// third-party `.clap` dynamic libraries.
#![deny(unsafe_code)]

pub mod automation;
pub mod bus;
#[cfg(feature = "clap")]
pub mod clap_host;
pub mod parameter;

pub use automation::ParameterAutomation;
pub use bus::{BusId, BusLayout, BusRouter, Route, SidechainDucker};
pub use parameter::{ParameterId, ParameterInfo, ParameterSet};

use tpt_av_audio_utils::{AudioBuffer, AudioError};

/// A plugin instance as seen by a host, independent of the plugin format.
///
/// Format backends (CLAP/VST3) will adapt their native instances to this
/// trait; `process` follows the same real-time contract as
/// `tpt_av_audio_core::AudioNode`.
pub trait HostedPlugin: Send {
    /// Stable plugin name for diagnostics and session serialization.
    fn name(&self) -> &str;

    /// The plugin's parameter surface.
    fn parameters(&self) -> &[ParameterInfo];

    /// Called by the host before the next block when parameters changed.
    fn set_parameter(&mut self, id: ParameterId, value: f32);

    /// Processes one buffer (allocation-free, lock-free, panic-free).
    fn process(&mut self, buffer: &mut AudioBuffer) -> Result<(), AudioError>;

    /// Transport/position sync: the host calls this each block so
    /// sample-accurate automation matches the timeline playhead.
    fn set_playhead(&mut self, _frame: u64) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parameter::ParameterId;

    struct FakePlugin {
        gain: f32,
        params: Vec<ParameterInfo>,
    }

    impl HostedPlugin for FakePlugin {
        fn name(&self) -> &str {
            "fake"
        }

        fn parameters(&self) -> &[ParameterInfo] {
            // Built per call; hosts snapshot the surface at instantiation.
            &self.params
        }

        fn set_parameter(&mut self, id: ParameterId, value: f32) {
            if id == ParameterId(1) {
                self.gain = value;
            }
        }

        fn process(&mut self, buffer: &mut AudioBuffer) -> Result<(), AudioError> {
            buffer.apply_gain(self.gain);
            Ok(())
        }
    }

    #[test]
    fn plugin_round_trip() {
        let mut p = FakePlugin {
            gain: 1.0,
            params: vec![ParameterInfo::new(ParameterId(1), "gain", 1.0, 0.0, 2.0)],
        };
        assert_eq!(p.parameters()[0].name, "gain");
        p.set_parameter(ParameterId(1), 0.5);

        let mut buf = AudioBuffer::new(1, 2);
        buf.data.iter_mut().for_each(|s| *s = 0.8);
        p.process(&mut buf).unwrap();
        assert!((buf.data[0] - 0.4).abs() < 1e-6);
    }
}
