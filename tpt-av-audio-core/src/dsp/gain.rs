//! Constant-gain node.

use tpt_av_audio_utils::{AudioBuffer, AudioError};

use crate::graph::AudioNode;

/// Scales the bus by a fixed linear gain.
#[derive(Debug)]
pub struct GainNode {
    gain: f32,
    channels: u16,
}

impl GainNode {
    /// Creates a gain node for a fixed channel count.
    pub fn new(gain: f32) -> Self {
        Self { gain, channels: 2 }
    }

    /// Overrides the channel count (defaults to stereo).
    pub fn with_channels(mut self, channels: u16) -> Self {
        self.channels = channels;
        self
    }

    /// Sets the gain (Main Thread; real-time safe atomic-free swap is not
    /// needed for a demo node — mutate only between renders).
    pub fn set_gain(&mut self, gain: f32) {
        self.gain = gain;
    }

    /// Current gain.
    pub fn gain(&self) -> f32 {
        self.gain
    }
}

impl AudioNode for GainNode {
    fn process(&mut self, buffer: &mut AudioBuffer) -> Result<(), AudioError> {
        buffer.apply_gain(self.gain);
        Ok(())
    }

    fn input_channels(&self) -> u16 {
        self.channels
    }

    fn output_channels(&self) -> u16 {
        self.channels
    }

    fn name(&self) -> &str {
        "gain"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scales_all_samples() {
        let mut node = GainNode::new(0.5);
        let mut buf = AudioBuffer::new(2, 2);
        buf.data.iter_mut().for_each(|s| *s = 0.8);
        node.process(&mut buf).unwrap();
        assert!(buf.data.iter().all(|&s| (s - 0.4).abs() < 1e-6));
    }
}
