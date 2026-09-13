//! PCM sample representations and conversions.
//!
//! The engine's canonical sample format is `f32` in the range `[-1.0, 1.0]`
//! (see [`crate::buffer::AudioBuffer`]). These helpers convert to and from
//! the integer widths commonly found in files and device formats.

/// A PCM sample width that can round-trip through the canonical `f32` format.
///
/// Conversions are lossy for integer widths (quantization), and `f32 -> i16`
/// clamps to the representable range instead of wrapping.
pub trait Sample: Copy + Default + 'static {
    /// The value representing digital silence.
    const SILENCE: Self;

    /// Converts from the canonical `f32` range `[-1.0, 1.0]`.
    fn from_f32(value: f32) -> Self;

    /// Converts into the canonical `f32` range `[-1.0, 1.0]`.
    fn to_f32(self) -> f32;
}

impl Sample for f32 {
    const SILENCE: Self = 0.0;

    #[inline]
    fn from_f32(value: f32) -> Self {
        value
    }

    #[inline]
    fn to_f32(self) -> f32 {
        self
    }
}

impl Sample for i16 {
    const SILENCE: Self = 0;

    #[inline]
    fn from_f32(value: f32) -> Self {
        let clamped = value.clamp(-1.0, 1.0);
        (clamped * 32_767.0) as i16
    }

    #[inline]
    fn to_f32(self) -> f32 {
        // Asymmetric scale keeps ±32767 mapped into [-1.0, 1.0).
        self as f32 / 32_768.0
    }
}

impl Sample for u8 {
    const SILENCE: Self = 128;

    #[inline]
    fn from_f32(value: f32) -> Self {
        let clamped = value.clamp(-1.0, 1.0);
        ((clamped * 127.0) + 128.0).round() as u8
    }

    #[inline]
    fn to_f32(self) -> f32 {
        (self as f32 - 128.0) / 128.0
    }
}

impl Sample for i32 {
    const SILENCE: Self = 0;

    #[inline]
    fn from_f32(value: f32) -> Self {
        // f64 intermediate: 2^31-1 is not representable in f32, and the f32
        // rounding would saturate the cast at i32::MIN for -1.0.
        let clamped = value.clamp(-1.0, 1.0) as f64;
        (clamped * 2_147_483_647.0) as i32
    }

    #[inline]
    fn to_f32(self) -> f32 {
        self as f32 / 2_147_483_648.0
    }
}

/// Converts a slice of any [`Sample`] width into the canonical `f32` range.
///
/// `dst` is resized to `src.len()` by the caller's contract: it must already
/// be the right length; this function writes in place and returns the number
/// of samples written.
pub fn convert_to_f32<S: Sample>(src: &[S], dst: &mut [f32]) -> usize {
    let n = src.len().min(dst.len());
    for (d, &s) in dst[..n].iter_mut().zip(src) {
        *d = s.to_f32();
    }
    n
}

/// Converts canonical `f32` samples into any [`Sample`] width.
pub fn convert_from_f32<S: Sample>(src: &[f32], dst: &mut [S]) -> usize {
    let n = src.len().min(dst.len());
    for (d, &s) in dst[..n].iter_mut().zip(src) {
        *d = S::from_f32(s);
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f32_round_trips() {
        assert_eq!(f32::to_f32(f32::from_f32(0.25)), 0.25);
        assert_eq!(f32::to_f32(f32::from_f32(-1.0)), -1.0);
    }

    #[test]
    fn i16_clamps_and_maps_silence() {
        assert_eq!(i16::from_f32(2.0), 32_767);
        assert_eq!(i16::from_f32(-2.0), -32_767);
        assert_eq!(i16::from_f32(0.0), 0);
        assert!((i16::to_f32(0) - 0.0).abs() < f32::EPSILON);
    }

    #[test]
    fn i16_round_trip_stays_close() {
        // Quantization plus the asymmetric /32768 scale bounds the error at
        // well under one LSB of 16-bit audio.
        let tolerance = 2.0 / 32_768.0;
        for v in [-1.0f32, -0.5, 0.0, 0.5, 0.999] {
            let back = i16::to_f32(i16::from_f32(v));
            assert!((back - v).abs() < tolerance, "v={v} back={back}");
        }
    }

    #[test]
    fn u8_is_offset_binary() {
        assert_eq!(u8::from_f32(0.0), 128);
        assert_eq!(u8::from_f32(1.0), 255);
        assert_eq!(u8::from_f32(-1.0), 1);
        assert!((u8::to_f32(128) - 0.0).abs() < f32::EPSILON);
    }

    #[test]
    fn i32_full_scale() {
        assert_eq!(i32::from_f32(1.0), 2_147_483_647);
        assert_eq!(i32::from_f32(-1.0), -2_147_483_647);
    }

    #[test]
    fn slice_conversions_respect_bounds() {
        let src = [0i16, 16_384, -16_384];
        let mut dst = [0.0f32; 4];
        let n = convert_to_f32(&src, &mut dst);
        assert_eq!(n, 3);
        assert!((dst[1] - 0.5).abs() < 0.001);

        let mut back = [0i16; 2];
        let n = convert_from_f32(&dst, &mut back);
        assert_eq!(n, 2);
        assert_eq!(back[1], 16_383);
    }
}
