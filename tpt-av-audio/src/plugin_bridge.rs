//! Adapts [`tpt_av_audio_plugin`]'s format-agnostic [`HostedPlugin`] onto
//! [`tpt_av_audio_core`]'s [`AudioNode`] graph.
//!
//! This lives in the umbrella crate rather than in `tpt-av-audio-core`
//! itself: neither `tpt-av-audio-core` nor `tpt-av-audio-plugin` depends on
//! the other today, and adding a plugin-crate dependency to the real-time
//! core crate would be an unwanted coupling for a feature only some
//! consumers need.

use tpt_av_audio_core::AudioNode;
use tpt_av_audio_plugin::HostedPlugin;
use tpt_av_audio_utils::{AudioBuffer, AudioError};

/// Wraps a [`HostedPlugin`] (e.g. [`tpt_av_audio_plugin::clap_host::ClapPluginNode`])
/// as an [`AudioNode`], so it can be inserted directly into an
/// [`tpt_av_audio_core::graph::AudioGraph`].
///
/// Channel counts are fixed at construction (v1: a plain in-place audio
/// effect, same channel count in and out — matching the graph's
/// summing-bus model).
pub struct HostedPluginAdapter<P: HostedPlugin> {
    plugin: P,
    channels: u16,
}

impl<P: HostedPlugin> HostedPluginAdapter<P> {
    /// Wraps `plugin` as an `AudioNode` processing `channels` channels.
    pub fn new(plugin: P, channels: u16) -> Self {
        Self { plugin, channels }
    }

    /// Returns the wrapped plugin, consuming the adapter.
    pub fn into_inner(self) -> P {
        self.plugin
    }
}

impl<P: HostedPlugin> AudioNode for HostedPluginAdapter<P> {
    fn process(&mut self, buffer: &mut AudioBuffer) -> Result<(), AudioError> {
        self.plugin.process(buffer)
    }

    fn input_channels(&self) -> u16 {
        self.channels
    }

    fn output_channels(&self) -> u16 {
        self.channels
    }

    fn name(&self) -> &str {
        self.plugin.name()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tpt_av_audio_plugin::parameter::{ParameterId, ParameterInfo};

    struct FakePlugin {
        gain: f32,
        params: Vec<ParameterInfo>,
    }

    impl HostedPlugin for FakePlugin {
        fn name(&self) -> &str {
            "fake"
        }

        fn parameters(&self) -> &[ParameterInfo] {
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
    fn adapter_forwards_process_and_metadata() {
        let plugin = FakePlugin {
            gain: 0.5,
            params: vec![ParameterInfo::new(ParameterId(1), "gain", 1.0, 0.0, 2.0)],
        };
        let mut node = HostedPluginAdapter::new(plugin, 2);
        assert_eq!(node.name(), "fake");
        assert_eq!(node.input_channels(), 2);
        assert_eq!(node.output_channels(), 2);

        let mut buffer = AudioBuffer::new(1, 2);
        buffer.data.iter_mut().for_each(|s| *s = 0.8);
        node.process(&mut buffer).unwrap();
        assert!((buffer.data[0] - 0.4).abs() < 1e-6);
    }
}
