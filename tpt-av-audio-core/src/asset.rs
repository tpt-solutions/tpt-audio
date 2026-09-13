//! Pre-allocated PCM caches per [`tpt_av_audio_timeline::AudioAsset`].
//!
//! [`AssetStore`] holds fully-decoded, pre-allocated PCM for every asset.
//! The Main Thread inserts (via the [`crate::decode`] pipeline); the Audio
//! Thread loads the map snapshot once per render — a single atomic refcount
//! bump — and reads `Arc<AssetPcm>` slices directly. No locks, no
//! allocation on the audio path.
//!
//! Replacement semantics: inserting an asset id twice supersedes the old
//! PCM (the audio thread may briefly keep reading the old `Arc`, which is
//! safe and drop happens off the RT thread).

use std::collections::HashMap;
use std::sync::Arc;

use arc_swap::ArcSwap;
use tpt_av_audio_timeline::AssetId;

/// Decoded, pre-allocated PCM for one asset (interleaved f32).
#[derive(Debug, Clone)]
pub struct AssetPcm {
    /// Sample rate of the stored PCM.
    pub sample_rate: u32,
    /// Interleaved channel count.
    pub channels: u16,
    /// Interleaved samples.
    pub data: Vec<f32>,
}

impl AssetPcm {
    /// Number of frames in this cache.
    pub fn frames(&self) -> u64 {
        if self.channels == 0 {
            return 0;
        }
        (self.data.len() / self.channels as usize) as u64
    }
}

/// Lock-free store of decoded assets, keyed by [`AssetId`].
#[derive(Default)]
pub struct AssetStore {
    map: ArcSwap<HashMap<AssetId, Arc<AssetPcm>>>,
}

impl AssetStore {
    /// Creates an empty store.
    pub fn new() -> Self {
        Self::default()
    }

    /// Inserts (or replaces) the PCM for `id`. Main Thread only.
    pub fn insert(&self, id: AssetId, pcm: AssetPcm) {
        let mut next = (**self.map.load()).clone();
        next.insert(id, Arc::new(pcm));
        self.map.store(Arc::new(next));
    }

    /// Removes an asset. Main Thread only.
    pub fn remove(&self, id: &AssetId) {
        let mut next = (**self.map.load()).clone();
        next.remove(id);
        self.map.store(Arc::new(next));
    }

    /// Number of cached assets.
    pub fn len(&self) -> usize {
        self.map.load().len()
    }

    /// Whether the store is empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Fetches one asset's PCM. Safe on the Audio Thread; clones the `Arc`.
    pub fn get(&self, id: &AssetId) -> Option<Arc<AssetPcm>> {
        self.map.load().get(id).cloned()
    }

    /// Loads the whole map snapshot once per render (cheaper than per-clip
    /// `get` calls when many clips share assets). Audio-Thread safe.
    pub fn snapshot(&self) -> Arc<HashMap<AssetId, Arc<AssetPcm>>> {
        Arc::clone(&self.map.load())
    }

    /// Total cached PCM frames across all assets (diagnostics).
    pub fn total_frames(&self) -> u64 {
        self.map.load().values().map(|pcm| pcm.frames()).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pcm(frames: u64, value: f32) -> AssetPcm {
        AssetPcm {
            sample_rate: 48_000,
            channels: 2,
            data: vec![value; (frames * 2) as usize],
        }
    }

    #[test]
    fn insert_get_round_trip() {
        let store = AssetStore::new();
        store.insert(AssetId(1), pcm(10, 0.5));
        let got = store.get(&AssetId(1)).unwrap();
        assert_eq!(got.frames(), 10);
        assert_eq!(got.data[0], 0.5);
        assert_eq!(store.len(), 1);
        assert_eq!(store.total_frames(), 10);
    }

    #[test]
    fn replace_supersedes_old_data() {
        let store = AssetStore::new();
        store.insert(AssetId(1), pcm(10, 0.5));
        let old = store.get(&AssetId(1)).unwrap();

        store.insert(AssetId(1), pcm(20, 0.9));
        let new = store.get(&AssetId(1)).unwrap();
        assert_eq!(new.frames(), 20);
        assert_eq!(new.data[0], 0.9);
        // Old Arc still valid (a render in flight keeps its view).
        assert_eq!(old.frames(), 10);
    }

    #[test]
    fn remove_works() {
        let store = AssetStore::new();
        store.insert(AssetId(7), pcm(1, 0.0));
        store.remove(&AssetId(7));
        assert!(store.get(&AssetId(7)).is_none());
        assert!(store.is_empty());
    }
}
