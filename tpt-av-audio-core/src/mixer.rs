//! Multi-track mixer: gain, pan, mute, and solo per track bus.

use tpt_av_audio_utils::{AudioBuffer, AudioError};

/// Balance-law pan gains for track strips: center is unity on both
/// channels (a centered fader never attenuates), hard left/right routes
/// everything to one channel. (Insert-effect panning uses the constant
/// power law in [`crate::dsp::pan`] instead.)
fn balance_gains(pan: f32) -> (f32, f32) {
    let pan = pan.clamp(-1.0, 1.0);
    if pan < 0.0 {
        (1.0, 1.0 + pan)
    } else {
        (1.0 - pan, 1.0)
    }
}

/// Per-track mixing controls (mirrors `timeline::Track` strip state).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TrackBus {
    /// Track volume (1.0 = unity).
    pub volume: f32,
    /// Track pan in `[-1.0, 1.0]` (stereo buses only).
    pub pan: f32,
    /// Muted tracks are silent.
    pub muted: bool,
    /// Solo: when any bus is soloed, non-soloed buses are silent.
    pub soloed: bool,
}

impl TrackBus {
    /// Unity-gain, centered, unmuted bus.
    pub fn unity() -> Self {
        Self {
            volume: 1.0,
            pan: 0.0,
            muted: false,
            soloed: false,
        }
    }
}

/// Sums N per-track buffers into one output bus.
#[derive(Debug, Default, Clone)]
pub struct TrackMixer {
    buses: Vec<TrackBus>,
}

impl TrackMixer {
    /// Creates a mixer with `count` unity buses.
    pub fn new(count: usize) -> Self {
        Self {
            buses: vec![TrackBus::unity(); count],
        }
    }

    /// Number of buses.
    pub fn len(&self) -> usize {
        self.buses.len()
    }

    /// Whether the mixer has no buses.
    pub fn is_empty(&self) -> bool {
        self.buses.is_empty()
    }

    /// Mutably borrow bus `i`.
    pub fn bus_mut(&mut self, i: usize) -> Option<&mut TrackBus> {
        self.buses.get_mut(i)
    }

    /// Borrow bus `i`.
    pub fn bus(&self, i: usize) -> Option<&TrackBus> {
        self.buses.get(i)
    }

    /// Sums `inputs[i]` into `out` through bus `i`.
    ///
    /// `any_solo` comes from the session state; when true, only soloed buses
    /// contribute. `out` is zeroed first, so mixing is idempotent per call.
    ///
    /// # Real-Time Safety
    ///
    /// Allocation-free: buses and buffers are preallocated by the caller.
    pub fn mix(
        &mut self,
        inputs: &[AudioBuffer],
        out: &mut AudioBuffer,
        any_solo: bool,
    ) -> Result<(), AudioError> {
        if inputs.len() > self.buses.len() {
            return Err(AudioError::InvalidConfig(format!(
                "{} track buffers but only {} buses (grow via prepare)",
                inputs.len(),
                self.buses.len()
            )));
        }
        out.clear();

        for (i, input) in inputs.iter().enumerate() {
            let bus = &self.buses[i];
            if bus.muted || (any_solo && !bus.soloed) {
                continue;
            }
            if input.channels != out.channels {
                return Err(AudioError::InvalidConfig(format!(
                    "track {i} has {} channels, output bus has {}",
                    input.channels, out.channels
                )));
            }

            let (gl, gr) = balance_gains(bus.pan);
            let channels = out.channels as usize;
            let gain = bus.volume;

            for (frame, (o, s)) in out
                .data
                .chunks_exact_mut(channels)
                .zip(input.data.chunks(channels))
                .enumerate()
            {
                let _ = frame;
                if channels >= 2 {
                    o[0] += s[0] * gain * gl;
                    o[1] += s[1] * gain * gr;
                    for (oc, sc) in o[2..].iter_mut().zip(&s[2..]) {
                        *oc += sc * gain;
                    }
                } else {
                    o[0] += s[0] * gain;
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stereo(value: f32) -> AudioBuffer {
        let mut b = AudioBuffer::new(1, 2);
        b.write_frame(0, &[value, value]).unwrap();
        b
    }

    #[test]
    fn sums_two_tracks() {
        let mut mixer = TrackMixer::new(2);
        let inputs = [stereo(0.5), stereo(0.5)];
        let mut out = AudioBuffer::new(1, 2);
        mixer.mix(&inputs, &mut out, false).unwrap();
        let mut frame = [0.0f32; 2];
        out.read_frame(0, &mut frame).unwrap();
        assert!((frame[0] - 1.0).abs() < 1e-6);
    }

    #[test]
    fn mute_and_solo() {
        let mut mixer = TrackMixer::new(2);
        mixer.bus_mut(0).unwrap().muted = true;
        let inputs = [stereo(1.0), stereo(1.0)];
        let mut out = AudioBuffer::new(1, 2);
        mixer.mix(&inputs, &mut out, false).unwrap();
        let mut frame = [0.0f32; 2];
        out.read_frame(0, &mut frame).unwrap();
        assert!((frame[0] - 1.0).abs() < 1e-6); // only track 2

        // Soloing track 1 (which is muted) silences everything.
        mixer.bus_mut(0).unwrap().soloed = true;
        mixer.mix(&inputs, &mut out, true).unwrap();
        out.read_frame(0, &mut frame).unwrap();
        assert!(frame[0] == 0.0);
    }

    #[test]
    fn pan_applies_to_stereo() {
        let mut mixer = TrackMixer::new(1);
        mixer.bus_mut(0).unwrap().pan = -1.0; // hard left
        let inputs = [stereo(1.0)];
        let mut out = AudioBuffer::new(1, 2);
        mixer.mix(&inputs, &mut out, false).unwrap();
        let mut frame = [0.0f32; 2];
        out.read_frame(0, &mut frame).unwrap();
        assert!((frame[0] - 1.0).abs() < 1e-4);
        assert!(frame[1].abs() < 1e-4);
    }

    #[test]
    fn buffer_count_must_fit_buses() {
        let mut mixer = TrackMixer::new(1);
        let inputs = [stereo(1.0), stereo(1.0)];
        let mut out = AudioBuffer::new(1, 2);
        assert!(mixer.mix(&inputs, &mut out, false).is_err());
    }

    #[test]
    fn balance_pan_center_is_unity() {
        let mut mixer = TrackMixer::new(1);
        mixer.bus_mut(0).unwrap().pan = 0.0;
        let inputs = [stereo(1.0)];
        let mut out = AudioBuffer::new(1, 2);
        mixer.mix(&inputs, &mut out, false).unwrap();
        let mut frame = [0.0f32; 2];
        out.read_frame(0, &mut frame).unwrap();
        assert!((frame[0] - 1.0).abs() < 1e-6);
        assert!((frame[1] - 1.0).abs() < 1e-6);
    }

    #[test]
    fn out_is_cleared_each_mix() {
        let mut mixer = TrackMixer::new(1);
        let mut out = AudioBuffer::new(1, 2);
        out.write_frame(0, &[9.0, 9.0]).unwrap();
        mixer.mix(&[stereo(1.0)], &mut out, false).unwrap();
        let mut frame = [0.0f32; 2];
        out.read_frame(0, &mut frame).unwrap();
        assert!((frame[0] - 1.0).abs() < 1e-6); // 9.0 was cleared, not summed
    }
}
