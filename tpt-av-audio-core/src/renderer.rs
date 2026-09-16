//! The timeline renderer: snapshot → PCM fetch → envelopes/fades → mix.
//!
//! Reads the latest [`SessionSnapshot`] from [`TimelineState`], renders
//! every active clip's PCM out of the [`AssetStore`], applies clip
//! envelopes, fades, resampling (linear inline when an asset's rate differs
//! from the session rate), and track strip state via [`TrackMixer`], then
//! advances the playhead.
//!
//! # Real-Time Safety
//!
//! `render` is allocation-free after `prepare`: per-track scratch buffers
//! are preallocated on the Main Thread and the snapshot/asset-map loads are
//! atomic `Arc` clones. If the session grows more tracks than the scratch
//! capacity, `render` returns an error rather than allocating (the owner
//! must call [`TimelineRenderer::prepare`] again from the Main Thread).

use std::sync::Arc;

use tpt_av_audio_timeline::{AssetId, Clip};
use tpt_av_audio_utils::{AudioBuffer, AudioError};

use crate::asset::AssetStore;
use crate::dsp::channels::{linear_resample_map_placed_into, SourcePlacement};
use crate::mixer::TrackMixer;
use crate::scheduler::TimelineState;

/// Renders the timeline into audio buffers.
pub struct TimelineRenderer {
    state: Arc<TimelineState>,
    assets: Arc<AssetStore>,
    /// Cached snapshot handle, reloaded once per render.
    snapshot: Option<Arc<crate::scheduler::SessionSnapshot>>,
    /// Preallocated per-track scratch buffers (Main Thread grown).
    track_buffers: Vec<AudioBuffer>,
    /// One clip-sized scratch: clips render here first, then ADD into the
    /// track buffer so overlapping clips mix (fades/crossfades shape the
    /// sum) instead of overwriting each other.
    clip_scratch: AudioBuffer,
    mixer: TrackMixer,
    playhead: u64,
}

impl TimelineRenderer {
    /// Creates a renderer over `state` and `assets`.
    pub fn new(state: Arc<TimelineState>, assets: Arc<AssetStore>) -> Self {
        Self {
            state,
            assets,
            snapshot: None,
            track_buffers: Vec::new(),
            clip_scratch: AudioBuffer::empty(),
            mixer: TrackMixer::new(0),
            playhead: 0,
        }
    }

    /// Reallocates scratch buffers for the current session shape. Main
    /// Thread only; call after adding tracks or changing buffer sizes.
    pub fn prepare(&mut self, buffer_frames: usize, channels: u16) {
        let session = &self.state.load_snapshot().session;
        let tracks = session.tracks.len();
        self.track_buffers
            .resize_with(tracks, || AudioBuffer::new(buffer_frames, channels));
        for buf in &mut self.track_buffers {
            buf.reset(buffer_frames, channels);
        }
        self.clip_scratch.reset(buffer_frames, channels);
        self.mixer = TrackMixer::new(tracks);
        self.sync_buses();
    }

    /// Copies track strip state (volume/pan/mute/solo) from the session into
    /// the mixer buses. Main Thread (called by `prepare`).
    fn sync_buses(&mut self) {
        let Some(snapshot) = self.snapshot.as_ref() else {
            return;
        };
        for (i, track) in snapshot.session.tracks.iter().enumerate() {
            if let Some(bus) = self.mixer.bus_mut(i) {
                bus.volume = track.volume;
                bus.pan = track.pan;
                bus.muted = track.muted;
                bus.soloed = track.soloed;
            }
        }
    }

    /// Current playhead position in session frames.
    pub fn position(&self) -> u64 {
        self.playhead
    }

    /// Moves the playhead (seek).
    pub fn seek(&mut self, frame: u64) {
        self.playhead = frame;
    }

    /// Renders the next buffer of audio from the timeline.
    ///
    /// # Real-Time Safety
    ///
    /// Called from the audio thread. Allocation-free: all buffers are
    /// preallocated; snapshot and asset-map loads are atomic refcount bumps.
    pub fn render(&mut self, buffer: &mut AudioBuffer) -> Result<(), AudioError> {
        // 1. Read timeline state (lock-free snapshot).
        let snapshot = self.state.load_snapshot();
        self.snapshot = Some(Arc::clone(&snapshot));
        let session = &snapshot.session;
        let any_solo = session.any_soloed();

        // Contract: `buffer` runs at the session sample rate. Buffers do not
        // carry a rate; asset-rate conversion happens inline below.

        if session.tracks.len() > self.track_buffers.len() {
            return Err(AudioError::BufferTooSmall {
                needed: session.tracks.len(),
                available: self.track_buffers.len(),
            });
        }

        let asset_map = self.assets.snapshot();
        let channels = buffer.channels;
        let frames = buffer.frames;
        let playhead = self.playhead;

        // 2. Render each track into its scratch buffer.
        for (track_idx, track) in session.tracks.iter().enumerate() {
            let scratch = &mut self.track_buffers[track_idx];
            scratch.clear();

            for clip in &track.clips {
                if clip.end_frame() <= playhead || clip.start_frame >= playhead + frames as u64 {
                    continue; // not active in this window
                }
                render_clip(
                    clip,
                    &asset_map,
                    session.sample_rate,
                    playhead,
                    frames,
                    channels,
                    &mut self.clip_scratch,
                    scratch,
                );
            }
        }

        // 3. Mix all tracks into the output bus (envelopes/fades were
        //    applied per-clip above; strip state applies here). Only the
        //    first N scratch buffers are live; extras stay silent if the
        //    session shrank without a prepare.
        self.sync_buses_rt();
        self.mixer.mix(
            &self.track_buffers[..session.tracks.len()],
            buffer,
            any_solo,
        )?;

        // 5. Advance playhead.
        self.playhead = playhead.saturating_add(frames as u64);
        Ok(())
    }

    /// Bus sync without touching the timeline snapshot twice; reads the
    /// cached snapshot (RT-safe: plain struct reads).
    fn sync_buses_rt(&mut self) {
        if let Some(snapshot) = self.snapshot.as_ref() {
            for (i, track) in snapshot.session.tracks.iter().enumerate() {
                if let Some(bus) = self.mixer.bus_mut(i) {
                    bus.volume = track.volume;
                    bus.pan = track.pan;
                    bus.muted = track.muted;
                    bus.soloed = track.soloed;
                }
            }
        }
    }
}

/// Renders one active clip into the track scratch buffer.
///
/// Allocation-free: reads PCM slices directly from the asset map and writes
/// into `scratch`.
#[allow(clippy::too_many_arguments)]
fn render_clip(
    clip: &Clip,
    asset_map: &std::collections::HashMap<AssetId, Arc<crate::asset::AssetPcm>>,
    session_rate: u32,
    playhead: u64,
    frames: usize,
    channels: u16,
    clip_scratch: &mut AudioBuffer,
    scratch: &mut AudioBuffer,
) {
    let Some(pcm) = asset_map.get(&clip.asset_id) else {
        return; // asset not (yet) decoded: render silence for this clip
    };

    // Overlap of [clip.start, clip.end) with [playhead, playhead + frames).
    let overlap_start = playhead.max(clip.start_frame);
    let overlap_end = (playhead + frames as u64).min(clip.end_frame());
    if overlap_end <= overlap_start {
        return;
    }
    let first_out_frame = (overlap_start - playhead) as usize;
    let out_frames = (overlap_end - overlap_start) as usize;

    // Source rate may differ from session rate; all clip positions are in
    // session frames.
    let ratio = if session_rate == 0 {
        1.0
    } else {
        pcm.sample_rate as f64 / session_rate as f64
    };

    // Source positioning: clip-local coordinates with optional loop
    // wrapping (the wrap makes source position piecewise-linear, so the
    // placement carries the loop region instead of a plain start offset).
    let clip_local_start = overlap_start - clip.start_frame;
    let placement = SourcePlacement {
        source_offset_frames: clip.source_offset,
        clip_local_start,
        loop_region: match (clip.loop_start, clip.loop_end) {
            (Some(ls), Some(le)) => Some((ls, le)),
            _ => None,
        },
    };

    // 1. Resample + channel-map into the clip scratch (mono→stereo fill,
    //    constant-power fold-down, proportional blocks, loop wrap).
    let scratch_channels = scratch.channels;
    clip_scratch.clear();
    linear_resample_map_placed_into(
        &pcm.data,
        pcm.channels,
        placement,
        ratio,
        &mut clip_scratch.data,
        scratch_channels,
        0,
        out_frames,
    );

    // 2. Shape the clip in isolation (envelopes + fades).
    apply_envelopes_and_fades(
        clip,
        overlap_start,
        out_frames,
        channels,
        &mut clip_scratch.data,
    );

    // 3. ADD the shaped clip into the track buffer: overlapping clips mix,
    //    so crossfades blend instead of the last clip winning.
    let start_sample = first_out_frame * channels as usize;
    let len = out_frames * channels as usize;
    for (dst, src) in scratch.data[start_sample..start_sample + len]
        .iter_mut()
        .zip(&clip_scratch.data[..len])
    {
        *dst += src;
    }
}

/// Multiplies the freshly-written clip region by clip volume/pan envelopes
/// and fade ramps. Allocation-free.
fn apply_envelopes_and_fades(
    clip: &Clip,
    timeline_start: u64,
    out_frames: usize,
    channels: u16,
    data: &mut [f32],
) {
    let channels = channels as usize;
    let clip_local_start = timeline_start - clip.start_frame;

    for i in 0..out_frames {
        let clip_local = clip_local_start + i as u64;
        let mut gain = 1.0f32;

        // Clip volume envelope (positions are clip-local frames).
        if let Some(env) = &clip.volume_envelope {
            gain *= env.value_at(clip_local);
        }

        // Fades (shape per the clip's curves; progress t runs 0→1 across
        // the ramp).
        if clip.fade_in_frames > 0 && clip_local < clip.fade_in_frames {
            let t = clip_local as f32 / clip.fade_in_frames as f32;
            gain *= clip.fade_in_curve.fade_in_gain(t);
        }
        if clip.fade_out_frames > 0 {
            let from_end = clip.duration_frames.saturating_sub(clip_local);
            if from_end <= clip.fade_out_frames {
                let t = 1.0 - from_end as f32 / clip.fade_out_frames as f32;
                gain *= clip.fade_out_curve.fade_out_gain(t);
            }
        }

        // Pan envelope (constant-power, stereo only).
        let (gl, gr) = match &clip.pan_envelope {
            Some(env) => crate::dsp::pan::pan_gains(env.value_at(clip_local)),
            None => (1.0, 1.0),
        };

        let base = i * channels;
        if channels >= 2 {
            data[base] *= gain * gl;
            data[base + 1] *= gain * gr;
            for ch in 2..channels {
                data[base + ch] *= gain;
            }
        } else if channels == 1 {
            data[base] *= gain;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::asset::{AssetPcm, AssetStore};
    use crate::scheduler::TimelineState;
    use tpt_av_audio_timeline::{
        Clip, ClipId, Envelope, EnvelopePoint, FadeCurve, InterpolationMethod, Session, TrackId,
    };

    fn store_with_tone(
        id: AssetId,
        frames: u64,
        channels: u16,
        rate: u32,
    ) -> (AssetStore, AssetPcm) {
        let pcm = AssetPcm {
            sample_rate: rate,
            channels,
            data: (0..frames * channels as u64)
                .map(|i| match i % channels as u64 {
                    0 => 1.0,
                    _ => 0.5,
                })
                .collect(),
        };
        let store = AssetStore::new();
        store.insert(id, pcm.clone());
        (store, pcm)
    }

    fn session_with_clip(session_rate: u32, clip: Clip) -> Session {
        let mut session = Session::new("r", session_rate);
        session.add_track("A");
        session.assets.push(tpt_av_audio_timeline::AudioAsset {
            id: clip.asset_id,
            file_path: "test.wav".into(),
            duration_frames: 1_000_000,
            sample_rate: session_rate,
            channels: 2,
        });
        session.track_mut(TrackId(1)).unwrap().insert_clip(clip);
        session
    }

    fn basic_clip() -> Clip {
        Clip {
            id: ClipId(1),
            asset_id: AssetId(1),
            start_frame: 0,
            source_offset: 0,
            duration_frames: 100,
            volume_envelope: None,
            pan_envelope: None,
            fade_in_frames: 0,
            fade_out_frames: 0,
            loop_start: None,
            loop_end: None,
            fade_in_curve: Default::default(),
            fade_out_curve: Default::default(),
        }
    }

    #[test]
    fn renders_clip_pcm_into_buffer() {
        let (store, _) = store_with_tone(AssetId(1), 200, 2, 48_000);
        let session = session_with_clip(48_000, basic_clip());
        let state = Arc::new(TimelineState::new(session));
        let mut renderer = TimelineRenderer::new(Arc::clone(&state), Arc::new(store));
        renderer.prepare(50, 2);

        let mut buf = AudioBuffer::new(50, 2);
        renderer.render(&mut buf).unwrap();

        // Left channel = 1.0, right = 0.5 straight from the asset.
        let mut frame = [0.0f32; 2];
        buf.read_frame(0, &mut frame).unwrap();
        assert!((frame[0] - 1.0).abs() < 1e-6);
        assert!((frame[1] - 0.5).abs() < 1e-6);
        assert_eq!(renderer.position(), 50);
    }

    #[test]
    fn silence_before_and_after_clip() {
        let (store, _) = store_with_tone(AssetId(1), 200, 2, 48_000);
        let mut clip = basic_clip();
        clip.start_frame = 100;
        clip.duration_frames = 50;
        let session = session_with_clip(48_000, clip);
        let state = Arc::new(TimelineState::new(session));
        let mut renderer = TimelineRenderer::new(Arc::clone(&state), Arc::new(store));
        renderer.prepare(50, 2);

        let mut buf = AudioBuffer::new(50, 2);
        renderer.render(&mut buf).unwrap(); // [0, 50): before clip
        assert!(buf.data.iter().all(|&s| s == 0.0));

        renderer.seek(100);
        renderer.render(&mut buf).unwrap(); // clip plays
        assert!(buf.data.iter().any(|&s| s != 0.0));

        renderer.seek(200);
        renderer.render(&mut buf).unwrap(); // after clip
        assert!(buf.data.iter().all(|&s| s == 0.0));
    }

    #[test]
    fn fade_in_shapes_amplitude() {
        let (store, _) = store_with_tone(AssetId(1), 200, 2, 48_000);
        let mut clip = basic_clip();
        clip.fade_in_frames = 100;
        let session = session_with_clip(48_000, clip);
        let state = Arc::new(TimelineState::new(session));
        let mut renderer = TimelineRenderer::new(Arc::clone(&state), Arc::new(store));
        renderer.prepare(10, 2);

        let mut buf = AudioBuffer::new(10, 2);
        renderer.render(&mut buf).unwrap();
        // Frame 0 = 0.0 (start of 100-frame fade-in), frame 9 = 0.09.
        assert!((buf.data[0] - 0.0).abs() < 1e-6);
        assert!((buf.data[9 * 2] - 0.09).abs() < 1e-5);
    }

    #[test]
    fn volume_envelope_follows_local_frames() {
        let (store, _) = store_with_tone(AssetId(1), 200, 2, 48_000);
        let mut clip = basic_clip();
        clip.volume_envelope = Some(Envelope::with_points(
            vec![
                EnvelopePoint {
                    frame: 0,
                    value: 0.0,
                },
                EnvelopePoint {
                    frame: 100,
                    value: 1.0,
                },
            ],
            InterpolationMethod::Linear,
        ));
        let session = session_with_clip(48_000, clip);
        let state = Arc::new(TimelineState::new(session));
        let mut renderer = TimelineRenderer::new(Arc::clone(&state), Arc::new(store));
        renderer.prepare(50, 2);

        let mut buf = AudioBuffer::new(50, 2);
        renderer.render(&mut buf).unwrap();
        // Mid-envelope at frame 49 ≈ 0.49 amplitude.
        assert!((buf.data[49 * 2] - 0.49).abs() < 1e-3);
    }

    #[test]
    fn resamples_asset_rate_to_session_rate() {
        // Asset at half the session rate → ratio 0.5; source sample 0 maps
        // to output 0, source sample 1 to output 2.
        let pcm = AssetPcm {
            sample_rate: 24_000,
            channels: 2,
            data: vec![1.0, 0.5, 0.8, 0.4, 0.6, 0.3],
        };
        let store = AssetStore::new();
        store.insert(AssetId(1), pcm);
        let session = session_with_clip(48_000, basic_clip());
        let state = Arc::new(TimelineState::new(session));
        let mut renderer = TimelineRenderer::new(Arc::clone(&state), Arc::new(store));
        renderer.prepare(4, 2);

        let mut buf = AudioBuffer::new(4, 2);
        renderer.render(&mut buf).unwrap();
        assert!((buf.data[0] - 1.0).abs() < 1e-6); // src pos 0.0 → frame 0
        assert!((buf.data[2] - 0.9).abs() < 1e-6); // src pos 0.5 = lerp(1.0, 0.8)
        assert!((buf.data[2 * 2] - 0.8).abs() < 1e-6); // src pos 1.0 → frame 1
    }

    #[test]
    fn mono_asset_upmixes_to_stereo_bus() {
        let pcm = crate::asset::AssetPcm {
            sample_rate: 48_000,
            channels: 1,
            data: vec![0.75; 100],
        };
        let store = crate::asset::AssetStore::new();
        store.insert(AssetId(1), pcm);
        let session = session_with_clip(48_000, basic_clip());
        let state = Arc::new(TimelineState::new(session));
        let mut renderer = TimelineRenderer::new(Arc::clone(&state), Arc::new(store));
        renderer.prepare(50, 2);

        let mut buf = AudioBuffer::new(50, 2);
        renderer.render(&mut buf).unwrap();
        assert!(buf.data.iter().all(|&s| (s - 0.75).abs() < 1e-6));
    }

    #[test]
    fn stereo_asset_folds_into_mono_bus() {
        let pcm = crate::asset::AssetPcm {
            sample_rate: 48_000,
            channels: 2,
            data: [1.0f32, 0.0].repeat(50), // interleaved L=1.0, R=0.0, 50 frames
        };
        let store = crate::asset::AssetStore::new();
        store.insert(AssetId(1), pcm);
        let session = session_with_clip(48_000, basic_clip());
        let state = Arc::new(TimelineState::new(session));
        let mut renderer = TimelineRenderer::new(Arc::clone(&state), Arc::new(store));
        renderer.prepare(50, 1);

        let mut buf = AudioBuffer::new(50, 1);
        renderer.render(&mut buf).unwrap();
        // Constant-power fold: (1 + 0) / sqrt(2).
        let expected = 1.0 / 2.0f32.sqrt();
        assert!(buf.data.iter().all(|&s| (s - expected).abs() < 1e-6));
    }

    #[test]
    fn loop_region_wraps_playback() {
        // Mono ramp asset 0..24; the clip plays 10 frames looping [4, 8).
        let pcm = crate::asset::AssetPcm {
            sample_rate: 48_000,
            channels: 1,
            data: (0..24).map(|i| i as f32).collect(),
        };
        let store = crate::asset::AssetStore::new();
        store.insert(AssetId(1), pcm);

        let mut session = Session::new("loop", 48_000);
        session.add_track("A");
        let clip_id = session.generate_clip_id();
        let clip = Clip::new(clip_id, AssetId(1), 0, 10).with_loop(4, 8);
        session.track_mut(TrackId(1)).unwrap().insert_clip(clip);

        let state = Arc::new(TimelineState::new(session));
        let mut renderer = TimelineRenderer::new(Arc::clone(&state), Arc::new(store));
        renderer.prepare(16, 1);

        let mut buf = AudioBuffer::new(16, 1);
        renderer.render(&mut buf).unwrap();
        let expected = [0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 4.0, 5.0];
        for (i, want) in expected.iter().enumerate() {
            assert!((buf.data[i] - want).abs() < 1e-6, "frame {i}");
        }
        // Frames past the clip duration (10) are silence.
        assert!(buf.data[10..].iter().all(|&s| s == 0.0));
    }

    #[test]
    fn equal_power_crossfade_blends_overlap() {
        // Two clips on one track overlapping by 10 frames; the left fades
        // out and the right fades in with equal-power curves over the
        // overlap. Asset A is constant 1.0, asset B constant 0.5.
        let store = crate::asset::AssetStore::new();
        store.insert(
            AssetId(1),
            crate::asset::AssetPcm {
                sample_rate: 48_000,
                channels: 1,
                data: vec![1.0; 100],
            },
        );
        store.insert(
            AssetId(2),
            crate::asset::AssetPcm {
                sample_rate: 48_000,
                channels: 1,
                data: vec![0.5; 100],
            },
        );

        let mut session = Session::new("xfade", 48_000);
        session.add_track("A");
        // Clip A: [0, 60), equal-power fade-out over its last 10 frames.
        // Clip B: [50, 150), equal-power fade-in over its first 10 frames.
        let a = Clip::new(session.generate_clip_id(), AssetId(1), 0, 60)
            .with_fades(0, 10)
            .with_fade_curves(FadeCurve::Linear, FadeCurve::EqualPower);
        let b = Clip::new(session.generate_clip_id(), AssetId(2), 50, 100)
            .with_fades(10, 0)
            .with_fade_curves(FadeCurve::EqualPower, FadeCurve::Linear);
        session.track_mut(TrackId(1)).unwrap().insert_clip(a);
        session.track_mut(TrackId(1)).unwrap().insert_clip(b);

        let state = Arc::new(TimelineState::new(session));
        let mut renderer = TimelineRenderer::new(Arc::clone(&state), Arc::new(store));
        renderer.prepare(200, 1);

        let mut buf = AudioBuffer::new(200, 1);
        renderer.render(&mut buf).unwrap();

        // Overlap is frames [50, 60). Midpoint frame 55: left is 5/10 into
        // its fade-out (cos 45°), right is 5/10 into its fade-in (sin 45°).
        let mid = 55;
        let gl = FadeCurve::EqualPower.fade_out_gain(0.5);
        let gr = FadeCurve::EqualPower.fade_in_gain(0.5);
        let expected = 1.0 * gl + 0.5 * gr;
        assert!(
            (buf.data[mid] - expected).abs() < 1e-5,
            "mid {}",
            buf.data[mid]
        );

        // First frame of the overlap: full left, ~zero right.
        assert!((buf.data[50] - 1.0).abs() < 1e-4);
        // Last overlap frame (59): left at cos(0.9·π/2) ≈ 0.156, right at
        // 0.5·sin(0.9·π/2) ≈ 0.494. Equal-power crossfades of correlated
        // material overshoot slightly near the middle — expected.
        let gl = FadeCurve::EqualPower.fade_out_gain(0.9);
        let gr = FadeCurve::EqualPower.fade_in_gain(0.9);
        assert!((buf.data[59] - (gl + 0.5 * gr)).abs() < 1e-4);
        // Past the overlap, pure right at full 0.5.
        assert!((buf.data[60] - 0.5).abs() < 1e-4);
    }

    #[test]
    fn missing_asset_renders_silence() {
        let store = AssetStore::new();
        let session = session_with_clip(48_000, basic_clip());
        let state = Arc::new(TimelineState::new(session));
        let mut renderer = TimelineRenderer::new(Arc::clone(&state), Arc::new(store));
        renderer.prepare(50, 2);

        let mut buf = AudioBuffer::new(50, 2);
        renderer.render(&mut buf).unwrap();
        assert!(buf.data.iter().all(|&s| s == 0.0));
    }

    #[test]
    fn mute_silences_track() {
        let (store, _) = store_with_tone(AssetId(1), 200, 2, 48_000);
        let mut session = session_with_clip(48_000, basic_clip());
        session.track_mut(TrackId(1)).unwrap().muted = true;
        let state = Arc::new(TimelineState::new(session));
        let mut renderer = TimelineRenderer::new(Arc::clone(&state), Arc::new(store));
        renderer.prepare(50, 2);

        let mut buf = AudioBuffer::new(50, 2);
        renderer.render(&mut buf).unwrap();
        assert!(buf.data.iter().all(|&s| s == 0.0));
    }

    #[test]
    fn solo_isolates_track() {
        let (store, _) = store_with_tone(AssetId(1), 200, 2, 48_000);
        let mut session = Session::new("solo", 48_000);
        session.add_track("quiet"); // has the clip, not soloed
        session.add_track("loud");
        session.track_mut(TrackId(2)).unwrap().soloed = true;
        session
            .track_mut(TrackId(1))
            .unwrap()
            .insert_clip(basic_clip());

        let state = Arc::new(TimelineState::new(session));
        let mut renderer = TimelineRenderer::new(Arc::clone(&state), Arc::new(store));
        renderer.prepare(50, 2);

        let mut buf = AudioBuffer::new(50, 2);
        renderer.render(&mut buf).unwrap();
        assert!(buf.data.iter().all(|&s| s == 0.0));
    }

    #[test]
    fn live_timeline_update_applies_on_next_render() {
        let (store, _) = store_with_tone(AssetId(1), 200, 2, 48_000);
        let session = session_with_clip(48_000, basic_clip());
        let state = Arc::new(TimelineState::new(session));
        let mut renderer = TimelineRenderer::new(Arc::clone(&state), Arc::new(store));
        renderer.prepare(50, 2);

        let mut buf = AudioBuffer::new(50, 2);
        renderer.render(&mut buf).unwrap();
        assert!(buf.data.iter().any(|&s| s != 0.0));

        // Main thread mutes the track mid-playback.
        let mut next = state.load_snapshot().session.clone();
        next.track_mut(TrackId(1)).unwrap().muted = true;
        state.update(next);

        renderer.render(&mut buf).unwrap();
        assert!(buf.data.iter().all(|&s| s == 0.0));
    }
}
