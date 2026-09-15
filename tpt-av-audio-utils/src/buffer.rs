//! The canonical interleaved audio buffer.
//!
//! [`AudioBuffer`] holds interleaved `f32` samples in `[-1.0, 1.0]` — the one
//! buffer type used across the real-time path in `tpt-av-audio-core`, the
//! I/O layer, and examples. It is deliberately plain: no generics, no trait
//! objects, nothing that would force an allocation inside an audio callback.

use crate::error::AudioError;

/// A buffer of interleaved `f32` audio samples.
#[derive(Debug, Clone, PartialEq)]
pub struct AudioBuffer {
    /// Interleaved samples, `frames * channels` in length.
    pub data: Vec<f32>,
    /// Number of frames (sample frames, i.e. samples-per-channel).
    pub frames: usize,
    /// Number of interleaved channels.
    pub channels: u16,
}

impl AudioBuffer {
    /// Creates a buffer of digital silence with room for `frames` frames of
    /// `channels` channels.
    pub fn new(frames: usize, channels: u16) -> Self {
        Self {
            data: vec![0.0; frames * channels as usize],
            frames,
            channels,
        }
    }

    /// Creates an empty buffer with zero capacity. Useful as a placeholder.
    pub fn empty() -> Self {
        Self {
            data: Vec::new(),
            frames: 0,
            channels: 0,
        }
    }

    /// Reuses the existing allocation, resizing to `frames` of `channels`
    /// channels and zeroing the contents.
    pub fn reset(&mut self, frames: usize, channels: u16) {
        self.data.clear();
        self.data.resize(frames * channels as usize, 0.0);
        self.frames = frames;
        self.channels = channels;
    }

    /// Fills the buffer with digital silence.
    pub fn clear(&mut self) {
        self.data.fill(0.0);
    }

    /// Total number of interleaved samples (`frames * channels`).
    pub fn len(&self) -> usize {
        self.frames * self.channels as usize
    }

    /// Whether the buffer holds no samples.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns `true` if `frame` is a valid frame index.
    #[inline]
    pub fn contains_frame(&self, frame: usize) -> bool {
        frame < self.frames
    }

    /// Copies one frame (one sample per channel) into `out`.
    ///
    /// Returns [`AudioError::BufferTooSmall`] if `out` is shorter than the
    /// channel count, and [`AudioError::InvalidConfig`] if `frame` is out of
    /// range. Panics never escape: the error path is the contract.
    pub fn read_frame(&self, frame: usize, out: &mut [f32]) -> Result<(), AudioError> {
        if frame >= self.frames {
            return Err(AudioError::InvalidConfig(format!(
                "frame {frame} out of range (buffer has {} frames)",
                self.frames
            )));
        }
        let ch = self.channels as usize;
        if out.len() < ch {
            return Err(AudioError::BufferTooSmall {
                needed: ch,
                available: out.len(),
            });
        }
        let start = frame * ch;
        out[..ch].copy_from_slice(&self.data[start..start + ch]);
        Ok(())
    }

    /// Writes one frame (one sample per channel) from `src`.
    ///
    /// Extra samples in `src` beyond the channel count are ignored.
    pub fn write_frame(&mut self, frame: usize, src: &[f32]) -> Result<(), AudioError> {
        if frame >= self.frames {
            return Err(AudioError::InvalidConfig(format!(
                "frame {frame} out of range (buffer has {} frames)",
                self.frames
            )));
        }
        let ch = self.channels as usize;
        if src.len() < ch {
            return Err(AudioError::BufferTooSmall {
                needed: ch,
                available: src.len(),
            });
        }
        let start = frame * ch;
        self.data[start..start + ch].copy_from_slice(&src[..ch]);
        Ok(())
    }

    /// Scales every sample by `gain`.
    pub fn apply_gain(&mut self, gain: f32) {
        for s in &mut self.data {
            *s *= gain;
        }
    }

    /// Mixes `other` into `self`, sample by sample, scaling `other` by `gain`.
    ///
    /// Only the overlapping range is mixed; if `other` is longer than `self`
    /// the remainder is ignored. Buffers must share the channel count.
    pub fn mix_from(&mut self, other: &AudioBuffer, gain: f32) -> Result<(), AudioError> {
        if self.channels != other.channels {
            return Err(AudioError::InvalidConfig(format!(
                "channel mismatch in mix: {} vs {}",
                self.channels, other.channels
            )));
        }
        let n = self.data.len().min(other.data.len());
        for (d, &s) in self.data[..n].iter_mut().zip(&other.data[..n]) {
            *d += s * gain;
        }
        Ok(())
    }

    /// The absolute peak sample (loudest instantaneous value). Returns 0.0
    /// for an empty buffer.
    pub fn peak(&self) -> f32 {
        self.data.iter().fold(0.0f32, |m, &s| m.max(s.abs()))
    }

    /// Root-mean-square level of the whole buffer (perceived loudness proxy).
    /// Returns 0.0 for an empty buffer.
    pub fn rms(&self) -> f32 {
        if self.data.is_empty() {
            return 0.0;
        }
        let sum: f32 = self.data.iter().map(|s| s * s).sum();
        (sum / self.data.len() as f32).sqrt()
    }

    /// The largest absolute peak across `buffers` — a quick headroom check
    /// before writing a mix to disk or a device.
    pub fn combined_peak<'a>(buffers: impl IntoIterator<Item = &'a AudioBuffer>) -> f32 {
        buffers.into_iter().fold(0.0f32, |m, b| m.max(b.peak()))
    }

    /// Splits a mono interleaved channel out into `out`, per frame.
    ///
    /// `channel` must be less than the channel count.
    pub fn read_channel(&self, channel: u16, out: &mut [f32]) -> Result<(), AudioError> {
        let ch = self.channels as usize;
        if channel as usize >= ch {
            return Err(AudioError::InvalidConfig(format!(
                "channel {channel} out of range (buffer has {ch} channels)"
            )));
        }
        if out.len() < self.frames {
            return Err(AudioError::BufferTooSmall {
                needed: self.frames,
                available: out.len(),
            });
        }
        for (frame, d) in out[..self.frames].iter_mut().enumerate() {
            *d = self.data[frame * ch + channel as usize];
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_is_silence() {
        let buf = AudioBuffer::new(4, 2);
        assert_eq!(buf.data.len(), 8);
        assert!(buf.data.iter().all(|&s| s == 0.0));
        assert_eq!(buf.len(), 8);
        assert!(!buf.is_empty());
    }

    #[test]
    fn frame_read_write_round_trip() {
        let mut buf = AudioBuffer::new(4, 2);
        buf.write_frame(2, &[0.25, -0.75]).unwrap();

        let mut out = [0.0f32; 2];
        buf.read_frame(2, &mut out).unwrap();
        assert_eq!(out, [0.25, -0.75]);
    }

    #[test]
    fn frame_errors_do_not_panic() {
        let buf = AudioBuffer::new(2, 2);
        assert!(buf.read_frame(5, &mut [0.0; 2]).is_err());
        assert!(buf.read_frame(0, &mut [0.0; 1]).is_err());
        let mut stereo = AudioBuffer::new(2, 2);
        assert!(stereo.write_frame(0, &[0.1]).is_err()); // 1 sample for 2 channels
        assert!(stereo.write_frame(9, &[0.1, 0.2]).is_err()); // frame out of range
    }

    #[test]
    fn mix_adds_with_gain() {
        let mut a = AudioBuffer::new(1, 2);
        a.write_frame(0, &[0.5, 0.5]).unwrap();
        let mut b = AudioBuffer::new(1, 2);
        b.write_frame(0, &[0.5, -0.5]).unwrap();

        a.mix_from(&b, 0.5).unwrap();
        let mut out = [0.0f32; 2];
        a.read_frame(0, &mut out).unwrap();
        assert!((out[0] - 0.75).abs() < 1e-6);
        assert!((out[1] - 0.25).abs() < 1e-6);
    }

    #[test]
    fn mix_rejects_channel_mismatch() {
        let mut a = AudioBuffer::new(1, 2);
        let b = AudioBuffer::new(1, 1);
        assert!(a.mix_from(&b, 1.0).is_err());
    }

    #[test]
    fn reset_reuses_and_zeroes() {
        let mut buf = AudioBuffer::new(4, 2);
        buf.data[0] = 1.0;
        buf.reset(2, 1);
        assert_eq!(buf.frames, 2);
        assert_eq!(buf.channels, 1);
        assert!(buf.data.iter().all(|&s| s == 0.0));
    }

    #[test]
    fn peak_rms_measure_levels() {
        let mut buf = AudioBuffer::new(4, 1);
        for (i, s) in [0.5f32, -0.25, 0.1, -1.0].iter().enumerate() {
            buf.data[i] = *s;
        }
        assert_eq!(buf.peak(), 1.0);
        let squares_sum = 0.25f32 + 0.0625 + 0.01 + 1.0;
        let expected: f32 = (squares_sum / 4.0).sqrt();
        assert!((buf.rms() - expected).abs() < 1e-6);

        assert_eq!(AudioBuffer::new(0, 2).peak(), 0.0);
        assert_eq!(AudioBuffer::new(0, 2).rms(), 0.0);
    }

    #[test]
    fn combined_peak_spans_buffers() {
        let mut a = AudioBuffer::new(1, 1);
        a.data[0] = 0.3;
        let mut b = AudioBuffer::new(1, 1);
        b.data[0] = -0.9;
        assert!((AudioBuffer::combined_peak([&a, &b]) - 0.9).abs() < 1e-6);
    }

    #[test]
    fn read_channel_deinterleaves() {
        let mut buf = AudioBuffer::new(2, 2);
        buf.write_frame(0, &[0.1, 0.2]).unwrap();
        buf.write_frame(1, &[0.3, 0.4]).unwrap();

        let mut ch = [0.0f32; 2];
        buf.read_channel(1, &mut ch).unwrap();
        assert_eq!(ch, [0.2, 0.4]);
        assert!(buf.read_channel(2, &mut ch).is_err());
    }
}
