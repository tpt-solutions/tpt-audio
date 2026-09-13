//! Position-tracking fade-in/fade-out node.

use tpt_av_audio_utils::{AudioBuffer, AudioError};

use crate::graph::AudioNode;

/// Applies a linear fade-in over `fade_in_frames` from the start of the
/// stream and a linear fade-out over the last `fade_out_frames` before
/// `total_frames`, tracking playback position across `process` calls.
#[derive(Debug)]
pub struct FadeNode {
    fade_in_frames: u64,
    fade_out_frames: u64,
    total_frames: u64,
    position: u64,
    channels: u16,
}

impl FadeNode {
    /// Creates a fade node for a stream of `total_frames` frames.
    pub fn new(fade_in_frames: u64, fade_out_frames: u64, total_frames: u64) -> Self {
        Self {
            fade_in_frames,
            fade_out_frames,
            total_frames,
            position: 0,
            channels: 2,
        }
    }

    /// Overrides the channel count (defaults to stereo).
    pub fn with_channels(mut self, channels: u16) -> Self {
        self.channels = channels;
        self
    }

    /// Current playhead inside the fade.
    pub fn position(&self) -> u64 {
        self.position
    }

    /// Reset the fade to the stream start.
    pub fn reset(&mut self) {
        self.position = 0;
    }

    /// Per-frame gain for absolute frame `f` (linear ramps).
    fn frame_gain(&self, f: u64) -> f32 {
        let mut g = 1.0f32;
        if self.fade_in_frames > 0 && f < self.fade_in_frames {
            g *= f as f32 / self.fade_in_frames as f32;
        }
        if self.fade_out_frames > 0 {
            let fade_start = self.total_frames.saturating_sub(self.fade_out_frames);
            if f >= fade_start {
                let into_fade = f - fade_start;
                g *= 1.0 - (into_fade as f32 / self.fade_out_frames as f32);
            }
        }
        g.clamp(0.0, 1.0)
    }
}

impl AudioNode for FadeNode {
    fn process(&mut self, buffer: &mut AudioBuffer) -> Result<(), AudioError> {
        let channels = buffer.channels as usize;
        for (i, sample) in buffer.data.iter_mut().enumerate() {
            let frame = self.position + (i / channels) as u64;
            *sample *= self.frame_gain(frame);
        }
        self.position += buffer.frames as u64;
        Ok(())
    }

    fn input_channels(&self) -> u16 {
        self.channels
    }

    fn output_channels(&self) -> u16 {
        self.channels
    }

    fn name(&self) -> &str {
        "fade"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fade_in_ramps_from_silence() {
        let mut node = FadeNode::new(4, 0, 100).with_channels(1);
        let mut buf = AudioBuffer::new(4, 1);
        buf.data.iter_mut().for_each(|s| *s = 1.0);
        node.process(&mut buf).unwrap();

        assert!((buf.data[0] - 0.0).abs() < 1e-6); // frame 0
        assert!((buf.data[1] - 0.25).abs() < 1e-6); // frame 1 of 4
        assert!((buf.data[3] - 0.75).abs() < 1e-6); // frame 3 of 4
        assert_eq!(node.position(), 4);
    }

    #[test]
    fn fade_out_reaches_silence_at_end() {
        let mut node = FadeNode::new(0, 4, 8).with_channels(1);
        // First 8 frames in two buffers.
        let mut buf = AudioBuffer::new(4, 1);
        buf.data.iter_mut().for_each(|s| *s = 1.0);
        node.process(&mut buf).unwrap();
        assert!(buf.data.iter().all(|&s| s == 1.0)); // fade starts at frame 4

        node.process(&mut buf).unwrap();
        // Frames 4..8: 4, 3, 2, 1 frames remain (including current) →
        // gains 1.0, 0.75, 0.5, 0.25. Silence lands at the first frame
        // past the clip end.
        assert!((buf.data[0] - 1.0).abs() < 1e-6);
        assert!((buf.data[1] - 0.75).abs() < 1e-6);
        assert!((buf.data[3] - 0.25).abs() < 1e-6);
    }

    #[test]
    fn no_fades_is_passthrough() {
        let mut node = FadeNode::new(0, 0, 100).with_channels(1);
        let mut buf = AudioBuffer::new(4, 1);
        buf.data.iter_mut().for_each(|s| *s = 0.9);
        node.process(&mut buf).unwrap();
        assert!(buf.data.iter().all(|&s| (s - 0.9).abs() < 1e-6));
    }
}
