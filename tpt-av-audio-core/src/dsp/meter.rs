//! Real-time metering: peak/RMS published through atomics for lock-free UI
//! polling.
//!
//! [`MeterNode`] is a pass-through [`AudioNode`] that measures the bus as it
//! flows by and publishes the levels to a shared [`Meter`]. A UI thread
//! reads `meter.peak()` / `meter.rms()` whenever it likes — the audio side
//! performs two atomic stores per buffer and nothing else.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use tpt_av_audio_utils::{AudioBuffer, AudioError};

use crate::graph::AudioNode;

/// Lock-free level readout shared between the audio thread and observers.
#[derive(Default)]
pub struct Meter {
    peak_bits: AtomicU32,
    rms_bits: AtomicU32,
}

impl Meter {
    /// Newest peak level (max-hold with the node's release).
    pub fn peak(&self) -> f32 {
        f32::from_bits(self.peak_bits.load(Ordering::Relaxed))
    }

    /// Newest RMS level.
    pub fn rms(&self) -> f32 {
        f32::from_bits(self.rms_bits.load(Ordering::Relaxed))
    }

    /// Resets both readings to silence.
    pub fn reset(&self) {
        self.peak_bits.store(0.0f32.to_bits(), Ordering::Relaxed);
        self.rms_bits.store(0.0f32.to_bits(), Ordering::Relaxed);
    }
}

/// Pass-through meter node. Create with [`MeterNode::new`], keep the
/// returned [`Arc<Meter>`] for reading, insert the node into an
/// [`crate::AudioGraph`] or call `process` directly.
pub struct MeterNode {
    channels: u16,
    meter: Arc<Meter>,
    /// Multiplier applied to the previous peak each buffer (release ballistics).
    /// 1.0 = infinite hold; 0.999 ≈ ~1 dB release per 100 buffers.
    release: f32,
}

impl MeterNode {
    /// Creates a meter for `channels` channels; returns the node and its
    /// shared readout.
    pub fn new(channels: u16) -> (Self, Arc<Meter>) {
        let meter = Arc::new(Meter::default());
        (
            Self {
                channels,
                meter: Arc::clone(&meter),
                release: 0.999,
            },
            meter,
        )
    }

    /// Sets the per-buffer peak release factor (see struct docs).
    pub fn set_release(&mut self, release: f32) {
        self.release = release.clamp(0.0, 1.0);
    }
}

impl AudioNode for MeterNode {
    fn process(&mut self, buffer: &mut AudioBuffer) -> Result<(), AudioError> {
        // Pass-through first: the meter never alters the signal.
        let peak = buffer.peak();
        let rms = buffer.rms();

        let prev_peak = f32::from_bits(self.meter.peak_bits.load(Ordering::Relaxed));
        let held = (prev_peak * self.release).max(peak);
        self.meter
            .peak_bits
            .store(held.to_bits(), Ordering::Relaxed);
        self.meter.rms_bits.store(rms.to_bits(), Ordering::Relaxed);
        Ok(())
    }

    fn input_channels(&self) -> u16 {
        self.channels
    }

    fn output_channels(&self) -> u16 {
        self.channels
    }

    fn name(&self) -> &str {
        "meter"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stereo(levels: &[f32]) -> AudioBuffer {
        let mut b = AudioBuffer::new(levels.len(), 2);
        for (i, &v) in levels.iter().enumerate() {
            b.write_frame(i, &[v, v]).unwrap();
        }
        b
    }

    #[test]
    fn publishes_peak_and_rms() {
        let (mut node, meter) = MeterNode::new(2);
        let mut buf = stereo(&[0.5, 0.25]);
        node.process(&mut buf).unwrap();

        // Pass-through: signal untouched.
        assert!((buf.data[0] - 0.5).abs() < 1e-6);
        assert!((meter.peak() - 0.5).abs() < 1e-6);
        let expected: f32 = ((0.25f32 + 0.0625) / 2.0).sqrt();
        assert!((meter.rms() - expected).abs() < 1e-6);
    }

    #[test]
    fn peak_holds_then_releases() {
        let (mut node, meter) = MeterNode::new(2);
        node.set_release(0.5); // halve the hold each buffer for a crisp test

        let mut loud = stereo(&[1.0]);
        node.process(&mut loud).unwrap();
        assert!((meter.peak() - 1.0).abs() < 1e-6);

        let mut quiet = stereo(&[0.1]);
        node.process(&mut quiet).unwrap();
        // Max-hold keeps ~0.5 (0.1 is below the released 0.5).
        assert!((meter.peak() - 0.5).abs() < 1e-6);

        node.process(&mut quiet).unwrap();
        assert!((meter.peak() - 0.25).abs() < 1e-6);
    }

    #[test]
    fn reset_returns_to_silence() {
        let (mut node, meter) = MeterNode::new(2);
        let mut buf = stereo(&[0.9]);
        node.process(&mut buf).unwrap();
        meter.reset();
        assert_eq!(meter.peak(), 0.0);
        assert_eq!(meter.rms(), 0.0);
    }
}
