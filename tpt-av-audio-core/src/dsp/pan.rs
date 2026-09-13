//! Equal-power stereo pan node.

use tpt_av_audio_utils::{AudioBuffer, AudioError};

use crate::graph::AudioNode;

/// Pans a stereo bus with the constant-power law:
/// left = cos(θ), right = sin(θ) where θ = (pan + 1) · π/4.
///
/// Hard left (pan = -1) is pure left with no level loss; center (0) is
/// each channel at cos(π/4) ≈ 0.707, preserving perceived loudness.
#[derive(Debug)]
pub struct PanNode {
    pan: f32,
    channels: u16,
}

impl PanNode {
    /// Creates a pan node for a stereo bus with `pan` in `[-1.0, 1.0]`.
    pub fn new(pan: f32) -> Self {
        Self {
            pan: pan.clamp(-1.0, 1.0),
            channels: 2,
        }
    }

    /// Current pan position.
    pub fn pan(&self) -> f32 {
        self.pan
    }

    /// Sets the pan position, clamped to `[-1.0, 1.0]`.
    pub fn set_pan(&mut self, pan: f32) {
        self.pan = pan.clamp(-1.0, 1.0);
    }
}

/// Constant-power gains for a pan position: `(left, right)`.
pub fn pan_gains(pan: f32) -> (f32, f32) {
    let pan = pan.clamp(-1.0, 1.0);
    let theta = (pan + 1.0) * std::f32::consts::FRAC_PI_4;
    (theta.cos(), theta.sin())
}

impl AudioNode for PanNode {
    fn process(&mut self, buffer: &mut AudioBuffer) -> Result<(), AudioError> {
        if buffer.channels < 2 {
            // Mono buses pass through; panning needs at least two channels.
            return Ok(());
        }
        let (gl, gr) = pan_gains(self.pan);
        for frame in buffer.data.chunks_exact_mut(buffer.channels as usize) {
            frame[0] *= gl;
            frame[1] *= gr;
        }
        Ok(())
    }

    fn input_channels(&self) -> u16 {
        self.channels
    }

    fn output_channels(&self) -> u16 {
        self.channels
    }

    fn name(&self) -> &str {
        "pan"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-4
    }

    #[test]
    fn center_preserves_power() {
        let (l, r) = pan_gains(0.0);
        let center = std::f32::consts::FRAC_1_SQRT_2;
        assert!(approx(l, center));
        assert!(approx(r, center));
        assert!(approx(l * l + r * r, 1.0));
    }

    #[test]
    fn hard_left_is_pure_left() {
        let (l, r) = pan_gains(-1.0);
        assert!(approx(l, 1.0));
        assert!(approx(r, 0.0));
    }

    #[test]
    fn hard_right_is_pure_right() {
        let (l, r) = pan_gains(1.0);
        assert!(approx(l, 0.0));
        assert!(approx(r, 1.0));
    }

    #[test]
    fn process_applies_per_channel() {
        let mut node = PanNode::new(1.0); // hard right
        let mut buf = AudioBuffer::new(1, 2);
        buf.write_frame(0, &[1.0, 1.0]).unwrap();
        node.process(&mut buf).unwrap();
        let mut out = [0.0f32; 2];
        buf.read_frame(0, &mut out).unwrap();
        assert!(approx(out[0], 0.0));
        assert!(approx(out[1], 1.0));
    }

    #[test]
    fn mono_passes_through() {
        let mut node = PanNode::new(-1.0);
        let mut buf = AudioBuffer::new(1, 1);
        buf.write_frame(0, &[0.5]).unwrap();
        node.process(&mut buf).unwrap();
        let mut out = [0.0f32; 1];
        buf.read_frame(0, &mut out).unwrap();
        assert!(approx(out[0], 0.5));
    }
}
