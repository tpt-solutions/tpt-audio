//! The [`Engine`]: one struct that wires the timeline, asset store,
//! decoder registry, and renderer together so applications need no
//! boilerplate plumbing.
//!
//! The engine owns the Main-Thread side of the world. `render` follows the
//! same real-time contract as [`TimelineRenderer::render`] — allocation-
//! free, lock-free, panic-free — so it can drive a stream callback or an
//! offline loop unchanged.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tpt_av_audio_core::{
    AssetPcm, AssetStore, DecodePool, DecodeRegistry, TimelineRenderer, TimelineState,
    WaveformOverview,
};
use tpt_av_audio_timeline::History;
use tpt_av_audio_timeline::{AssetId, AudioAsset, Edit, Session};
use tpt_av_audio_utils::AudioBuffer;

/// The ready-to-use audio engine: timeline state + asset cache + renderer.
pub struct Engine {
    state: Arc<TimelineState>,
    store: Arc<AssetStore>,
    registry: Arc<DecodeRegistry>,
    renderer: TimelineRenderer,
    pool: Option<DecodePool>,
    buffer_frames: usize,
    channels: u16,
    overview_cache: HashMap<(AssetId, u32), Arc<WaveformOverview>>,
    history: History,
}

impl Engine {
    /// Creates an engine for `session`, with per-render buffers of
    /// `buffer_frames` frames × `channels` channels.
    pub fn new(
        session: Session,
        buffer_frames: usize,
        channels: u16,
    ) -> Result<Self, tpt_av_audio_utils::AudioError> {
        let state = Arc::new(TimelineState::new(session));
        let store = Arc::new(AssetStore::new());
        let registry = Arc::new(DecodeRegistry::with_builtins());
        let renderer = TimelineRenderer::new(Arc::clone(&state), Arc::clone(&store));
        let mut engine = Self {
            state,
            store,
            registry,
            renderer,
            pool: None,
            buffer_frames,
            channels,
            overview_cache: HashMap::new(),
            history: History::new(100),
        };
        engine.prepare_renderer();
        Ok(engine)
    }

    fn prepare_renderer(&mut self) {
        self.renderer.prepare(self.buffer_frames, self.channels);
    }

    /// The buffer geometry the engine renders into.
    pub fn buffer_geometry(&self) -> (usize, u16) {
        (self.buffer_frames, self.channels)
    }

    /// A clone of the currently published session (Main Thread).
    pub fn session(&self) -> Session {
        self.state.load_snapshot().session.clone()
    }

    /// Publishes a new session and re-prepares the renderer for its shape.
    pub fn set_session(&mut self, session: Session) {
        self.state.update(session);
        self.prepare_renderer();
    }

    /// The shared asset store (inspect caches, insert synthetic PCM).
    pub fn store(&self) -> &AssetStore {
        &self.store
    }

    /// Decodes `path` on this thread, registers it as an asset in the
    /// session, caches the PCM, and returns the new [`AssetId`].
    ///
    /// Use [`Engine::queue_asset`] instead when the file is large and the
    /// UI must not block.
    pub fn load_asset(
        &mut self,
        path: impl AsRef<Path>,
    ) -> Result<AssetId, tpt_av_audio_utils::AudioError> {
        let path = path.as_ref();
        let decoded = tpt_av_audio_core::decode::decode_file(&self.registry, path)?;
        let asset_id = self.register_asset_in_session(path, &decoded);

        self.store.insert(
            asset_id,
            AssetPcm {
                sample_rate: decoded.sample_rate,
                channels: decoded.channels,
                data: decoded.data,
            },
        );
        Ok(asset_id)
    }

    /// Starts `workers` background decode threads. After this,
    /// [`Engine::queue_asset`] decodes off-thread and
    /// [`Engine::completed_assets`] reports progress.
    pub fn spawn_decode_workers(
        &mut self,
        workers: usize,
    ) -> Result<(), tpt_av_audio_utils::AudioError> {
        self.pool = Some(DecodePool::new(
            workers,
            Arc::clone(&self.registry),
            Arc::clone(&self.store),
        )?);
        Ok(())
    }

    /// Registers the asset in the session and queues a background decode.
    /// The clip renders silence until the decode completes (poll
    /// [`Engine::completed_assets`]). Requires
    /// [`Engine::spawn_decode_workers`] first.
    pub fn queue_asset(
        &mut self,
        path: impl AsRef<Path>,
    ) -> Result<AssetId, tpt_av_audio_utils::AudioError> {
        let path = path.as_ref();
        // Register the asset with header facts unknown for now (0s); the
        // background decode fills the PCM cache and callers can re-check.
        let asset_id = self.mint_asset_id();
        self.add_asset_to_session(AudioAsset {
            id: asset_id,
            file_path: path.to_path_buf(),
            duration_frames: 0,
            sample_rate: 0,
            channels: 0,
        });

        let pool = self
            .pool
            .as_mut()
            .ok_or(tpt_av_audio_utils::AudioError::InvalidConfig(
                "queue_asset requires spawn_decode_workers() first".into(),
            ))?;
        pool.submit(asset_id, path);
        Ok(asset_id)
    }

    /// Drains background-decode completion events (see [`Engine::queue_asset`]).
    pub fn completed_assets(&mut self) -> Vec<AssetId> {
        match &self.pool {
            Some(pool) => pool.try_poll_completed(),
            None => Vec::new(),
        }
    }

    /// Inserts caller-provided PCM as a registered asset (synthesis,
    /// procedural audio, or PCM from another decoder).
    pub fn insert_pcm(
        &mut self,
        path_hint: impl Into<PathBuf>,
        sample_rate: u32,
        channels: u16,
        data: Vec<f32>,
    ) -> Result<AssetId, tpt_av_audio_utils::AudioError> {
        let asset_id = self.mint_asset_id();
        self.add_asset_to_session(AudioAsset {
            id: asset_id,
            file_path: path_hint.into(),
            duration_frames: if channels == 0 {
                0
            } else {
                (data.len() / channels as usize) as u64
            },
            sample_rate,
            channels,
        });
        self.store.insert(
            asset_id,
            AssetPcm {
                sample_rate,
                channels,
                data,
            },
        );
        Ok(asset_id)
    }

    fn mint_asset_id(&mut self) -> AssetId {
        let mut session = self.session();
        let id = session.ids.next_asset_id();
        // Reserve the id in the published snapshot too so a later
        // set_session cannot collide.
        session.ids.reserve_asset_id(id);
        self.state.update(session);
        id
    }

    fn register_asset_in_session(
        &mut self,
        path: &Path,
        decoded: &tpt_av_audio_core::decode::DecodedAudio,
    ) -> AssetId {
        let asset_id = self.mint_asset_id();
        self.add_asset_to_session(AudioAsset {
            id: asset_id,
            file_path: path.to_path_buf(),
            duration_frames: decoded.frames(),
            sample_rate: decoded.sample_rate,
            channels: decoded.channels,
        });
        asset_id
    }

    fn add_asset_to_session(&mut self, asset: AudioAsset) {
        let mut session = self.session();
        if !session.assets.iter().any(|a| a.id == asset.id) {
            session.assets.push(asset);
            self.state.update(session);
        }
    }

    /// Renders the next buffer of mixed timeline audio.
    ///
    /// # Real-Time Safety
    ///
    /// Allocation-free, lock-free, panic-free (see [`TimelineRenderer::render`]).
    pub fn render(
        &mut self,
        buffer: &mut AudioBuffer,
    ) -> Result<(), tpt_av_audio_utils::AudioError> {
        self.renderer.render(buffer)
    }

    /// Current playhead, in session frames.
    pub fn position(&self) -> u64 {
        self.renderer.position()
    }

    /// Seeks the playhead, in session frames.
    pub fn seek(&mut self, frame: u64) {
        self.renderer.seek(frame);
    }

    /// Builds (or fetches from cache) a [`WaveformOverview`] for an asset —
    /// min/max peak buckets a UI can draw directly. Main Thread only.
    pub fn overview(
        &mut self,
        asset_id: AssetId,
        buckets_per_second: u32,
    ) -> Result<Arc<WaveformOverview>, tpt_av_audio_utils::AudioError> {
        let key = (asset_id, buckets_per_second);
        if let Some(cached) = self.overview_cache.get(&key) {
            return Ok(Arc::clone(cached));
        }
        let pcm = self
            .store
            .get(&asset_id)
            .ok_or(tpt_av_audio_utils::AudioError::AssetNotFound(asset_id.0))?;
        let overview = Arc::new(WaveformOverview::from_pcm(&pcm, buckets_per_second));
        self.overview_cache.insert(key, Arc::clone(&overview));
        Ok(overview)
    }

    /// Applies an undoable timeline edit (insert/remove/move/split/crossfade).
    /// The session republish happens here, so the next render reflects it.
    pub fn apply_edit(
        &mut self,
        edit: Box<dyn Edit>,
    ) -> Result<(), tpt_av_audio_utils::AudioError> {
        let mut session = self.session();
        self.history.apply(edit, &mut session)?;
        self.set_session(session);
        Ok(())
    }

    /// Undoes the most recent edit. Returns `false` when history is empty.
    pub fn undo(&mut self) -> Result<bool, tpt_av_audio_utils::AudioError> {
        let mut session = self.session();
        let undone = self.history.undo(&mut session)?;
        if undone {
            self.set_session(session);
        }
        Ok(undone)
    }

    /// Redoes the most recently undone edit. Returns `false` when there is
    /// nothing to redo.
    pub fn redo(&mut self) -> Result<bool, tpt_av_audio_utils::AudioError> {
        let mut session = self.session();
        let redone = self.history.redo(&mut session)?;
        if redone {
            self.set_session(session);
        }
        Ok(redone)
    }

    /// Whether an undo is available.
    pub fn can_undo(&self) -> bool {
        self.history.can_undo()
    }

    /// Whether a redo is available.
    pub fn can_redo(&self) -> bool {
        self.history.can_redo()
    }

    /// Shuts the decode pool down, waiting for queued jobs.
    pub fn finish_decode_workers(&mut self) {
        if let Some(pool) = self.pool.take() {
            pool.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tpt_av_audio_timeline::Clip;

    fn write_wav(path: &Path, frames: usize) {
        let spec = hound::WavSpec {
            channels: 2,
            sample_rate: 48_000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::create(path, spec).unwrap();
        for i in 0..frames * 2 {
            writer
                .write_sample(if i % 2 == 0 { 16_000 } else { -16_000 })
                .unwrap();
        }
        writer.finalize().unwrap();
    }

    #[test]
    fn load_asset_registers_header_facts() {
        let dir = std::env::temp_dir().join("tpt-av-audio-engine-tests");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("engine_asset.wav");
        write_wav(&path, 1_000);

        let mut engine = Engine::new(Session::new("e", 48_000), 256, 2).unwrap();
        let asset = engine.load_asset(&path).unwrap();

        let session = engine.session();
        let asset_obj = session.asset(asset).unwrap();
        assert_eq!(asset_obj.duration_frames, 1_000);
        assert_eq!(asset_obj.sample_rate, 48_000);
        assert_eq!(asset_obj.channels, 2);
    }

    #[test]
    fn engine_renders_loaded_clip() {
        let dir = std::env::temp_dir().join("tpt-av-audio-engine-tests");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("engine_render.wav");
        write_wav(&path, 500);

        let mut engine = Engine::new(Session::new("e", 48_000), 256, 2).unwrap();
        let asset = engine.load_asset(&path).unwrap();

        let mut session = engine.session();
        let track = session.add_track("vocals");
        let clip_id = session.generate_clip_id();
        session
            .track_mut(track)
            .unwrap()
            .insert_clip(Clip::new(clip_id, asset, 0, 400));
        engine.set_session(session);

        let mut out = AudioBuffer::new(256, 2);
        engine.render(&mut out).unwrap();
        assert!(out.data.iter().any(|&s| s != 0.0));
        assert_eq!(engine.position(), 256);
    }

    #[test]
    fn queued_asset_decodes_in_background() {
        let dir = std::env::temp_dir().join("tpt-av-audio-engine-tests");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("engine_bg.wav");
        write_wav(&path, 42);

        let mut engine = Engine::new(Session::new("e", 48_000), 128, 2).unwrap();
        engine.spawn_decode_workers(1).unwrap();
        let asset = engine.queue_asset(&path).unwrap();

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while engine.completed_assets().is_empty() && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        engine.finish_decode_workers();

        assert!(engine.store().get(&asset).is_some());
        assert_eq!(engine.store().get(&asset).unwrap().frames(), 42);
    }

    #[test]
    fn engine_undo_redo_round_trips_edits() {
        let mut engine = Engine::new(Session::new("e", 48_000), 64, 2).unwrap();
        let asset = engine
            .insert_pcm("synth://ramp", 48_000, 2, vec![0.5; 9_600])
            .unwrap();

        let mut session = engine.session();
        let track = session.add_track("clip");
        let clip = Clip::new(session.generate_clip_id(), asset, 0, 1_000);
        let clip_id = clip.id;
        engine.set_session(session); // publish the track first
        let edit = Box::new(tpt_av_audio_timeline::InsertClipEdit::new(track, clip));
        engine.apply_edit(edit).unwrap();
        assert!(engine.can_undo());

        engine.undo().unwrap();
        assert!(engine.session().track(track).unwrap().clips.is_empty());
        assert!(engine.can_redo());

        engine.redo().unwrap();
        assert_eq!(
            engine
                .session()
                .track(track)
                .unwrap()
                .clip(clip_id)
                .unwrap()
                .duration_frames,
            1_000
        );
    }

    #[test]
    fn overview_builds_and_caches() {
        let mut engine = Engine::new(Session::new("e", 48_000), 64, 2).unwrap();
        let asset = engine
            .insert_pcm(
                "synth://ramp",
                48_000,
                1,
                (0..48_000).map(|i| i as f32 / 48_000.0).collect(),
            )
            .unwrap();

        let o1 = engine.overview(asset, 100).unwrap();
        let o2 = engine.overview(asset, 100).unwrap();
        assert!(Arc::ptr_eq(&o1, &o2), "second call must hit the cache");
        assert_eq!(o1.bucket_count(), 100);
        // Ramp: last bucket tops out near full scale.
        let (_, max) = o1.min_max(99);
        assert!(max > 0.95);

        assert!(engine
            .overview(tpt_av_audio_timeline::AssetId(999), 100)
            .is_err());
    }

    #[test]
    fn insert_pcm_supports_synthesis() {
        let mut engine = Engine::new(Session::new("e", 48_000), 64, 1).unwrap();
        let sine: Vec<f32> = (0..4_800)
            .map(|i| (2.0 * std::f32::consts::PI * 440.0 * i as f32 / 48_000.0).sin())
            .collect();
        let asset = engine.insert_pcm("synth://sine", 48_000, 1, sine).unwrap();
        assert!(engine.store().get(&asset).is_some());

        let mut session = engine.session();
        let track = session.add_track("sine");
        let clip_id = session.generate_clip_id();
        session
            .track_mut(track)
            .unwrap()
            .insert_clip(Clip::new(clip_id, asset, 0, 4_800));
        engine.set_session(session);

        let mut out = AudioBuffer::new(64, 1);
        engine.render(&mut out).unwrap();
        assert!(out.data.iter().any(|&s| s != 0.0));
    }
}
