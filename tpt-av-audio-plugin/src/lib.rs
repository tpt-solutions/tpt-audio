//! # tpt-av-audio-plugin
//!
//! Plugin hosting foundation for the `tpt-av-audio-*` engine: parameter
//! automation wired into the timeline [`Envelope`](tpt_av_audio_timeline::Envelope)
//! model, plus bus routing and side-chaining.
//!
//! ## Hosting status
//!
//! VST3 and CLAP *hosting* backends are future work. Note that `nih-plug`
//! — named in the original plan — is a plugin **development** framework and
//! deliberately does not expose a hosting API, so it cannot provide the
//! host side. The realistic paths are:
//!
//! - **CLAP**: `clack-host` (pure Rust, permissively licensed), or
//! - **VST3**: `vst3-sys`-based COM hosting (Windows) plus the VST3 SDK
//!   module scanning on macOS/Linux.
//!
//! Everything in this crate is backend-agnostic and already useful: hosts
//! describe plugins with [`HostedPlugin`], automate parameters through
//! [`ParameterAutomation`] (envelope-driven, timeline-frame based), and
//! wire side-chains and buses with [`bus`].

// Hosting foundation: no unsafe anywhere.
#![forbid(unsafe_code)]

pub mod automation;
pub mod bus;
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
