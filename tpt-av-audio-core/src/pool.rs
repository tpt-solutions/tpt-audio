//! Background thread pool for decoding and caching assets.
//!
//! [`DecodePool`] owns N worker threads. The Main Thread submits
//! [`DecodePool::submit`] jobs (asset id + path); a worker opens a decoder
//! through a [`DecodeRegistry`], decodes to PCM, and inserts the result
//! into the shared [`AssetStore`]. Completion events land on a bounded
//! channel the Main Thread can drain (for progress UI).
//!
//! The audio thread never touches this module — it only reads the
//! pre-populated `AssetStore`.

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::thread;

use tpt_av_audio_timeline::AssetId;

use crate::asset::{AssetPcm, AssetStore};
use crate::decode::DecodeRegistry;

/// A decode job.
struct DecodeJob {
    asset_id: AssetId,
    path: PathBuf,
}

/// Background decode pool feeding an [`AssetStore`].
pub struct DecodePool {
    tx: Sender<DecodeJob>,
    workers: Vec<thread::JoinHandle<()>>,
    events: Receiver<AssetId>,
}

impl DecodePool {
    /// Spawns `workers` background threads decoding through `registry` into
    /// `store`.
    pub fn new(
        workers: usize,
        registry: Arc<DecodeRegistry>,
        store: Arc<AssetStore>,
    ) -> Result<Self, tpt_av_audio_utils::AudioError> {
        let workers = workers.max(1);
        let (job_tx, job_rx) = mpsc::channel::<DecodeJob>();
        let (event_tx, event_rx) = mpsc::channel::<AssetId>();
        let job_rx = Arc::new(std::sync::Mutex::new(job_rx));

        let handles: Vec<_> = (0..workers)
            .map(|i| {
                let job_rx = Arc::clone(&job_rx);
                let event_tx = event_tx.clone();
                let registry = Arc::clone(&registry);
                let store = Arc::clone(&store);
                thread::Builder::new()
                    .name(format!("tpt-audio-decode-{i}"))
                    .spawn(move || loop {
                        let job = {
                            // Poison-tolerant: a panicking worker must not
                            // wedge the pool's job queue.
                            let rx = job_rx.lock().unwrap_or_else(|e| e.into_inner());
                            match rx.recv() {
                                Ok(job) => job,
                                Err(_) => break, // all senders dropped
                            }
                        };
                        match crate::decode::decode_file(&registry, &job.path) {
                            Ok(decoded) => {
                                store.insert(
                                    job.asset_id,
                                    AssetPcm {
                                        sample_rate: decoded.sample_rate,
                                        channels: decoded.channels,
                                        data: decoded.data,
                                    },
                                );
                                let _ = event_tx.send(job.asset_id);
                            }
                            Err(e) => {
                                log::warn!(
                                    "decode failed for asset {} ({}): {e}",
                                    job.asset_id.0,
                                    job.path.display()
                                );
                            }
                        }
                    })
                    .map_err(|e| {
                        tpt_av_audio_utils::AudioError::Backend(format!(
                            "failed to spawn decode worker: {e}"
                        ))
                    })
            })
            .collect::<Result<Vec<_>, _>>()?;

        Ok(Self {
            tx: job_tx,
            workers: handles,
            events: event_rx,
        })
    }

    /// Submits a decode job. The asset appears in the store once a worker
    /// finishes (until then the renderer renders silence for its clips).
    pub fn submit(&self, asset_id: AssetId, path: impl Into<PathBuf>) {
        let _ = self.tx.send(DecodeJob {
            asset_id,
            path: path.into(),
        });
    }

    /// Drains completed-asset events without blocking.
    pub fn try_poll_completed(&self) -> Vec<AssetId> {
        let mut done = Vec::new();
        while let Ok(id) = self.events.try_recv() {
            done.push(id);
        }
        done
    }

    /// Blocks until all queued jobs finish, then shuts the pool down.
    pub fn join(mut self) {
        drop(self.tx); // workers exit when the job channel drains
        for handle in self.workers.drain(..) {
            let _ = handle.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decode::DecodeRegistry;
    use tpt_av_audio_utils::wav::{SampleFormat, WavSpec, WavWriter};

    fn write_wav(path: &PathBuf, frames: usize) {
        let spec = WavSpec {
            channels: 2,
            sample_rate: 48_000,
            bits_per_sample: 16,
            sample_format: SampleFormat::Int,
        };
        let mut writer = WavWriter::create(path, spec).unwrap();
        for i in 0..frames * 2 {
            writer.write_sample((i % 1000) as i16).unwrap();
        }
        writer.finalize().unwrap();
    }

    #[test]
    fn pool_decodes_into_store_and_reports_completion() {
        let dir = std::env::temp_dir().join("tpt-av-audio-core-pool-tests");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("pool_test.wav");
        write_wav(&path, 100);

        let store = Arc::new(AssetStore::new());
        let pool = DecodePool::new(
            2,
            Arc::new(DecodeRegistry::with_builtins()),
            Arc::clone(&store),
        )
        .unwrap();

        pool.submit(AssetId(1), &path);
        pool.submit(AssetId(2), &path);
        pool.join();

        // try_poll_completed must be called before join drops the workers…
        // (events already delivered are still readable after join.)
        assert_eq!(store.get(&AssetId(1)).unwrap().frames(), 100);
        assert_eq!(store.get(&AssetId(2)).unwrap().frames(), 100);
    }

    #[test]
    fn missing_file_is_logged_not_fatal() {
        let store = Arc::new(AssetStore::new());
        let pool = DecodePool::new(
            1,
            Arc::new(DecodeRegistry::with_builtins()),
            Arc::clone(&store),
        )
        .unwrap();
        pool.submit(AssetId(9), "/definitely/not/here.wav");
        pool.join();
        assert!(store.get(&AssetId(9)).is_none());
    }

    #[test]
    fn events_drain_after_decode() {
        let dir = std::env::temp_dir().join("tpt-av-audio-core-pool-tests");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("pool_event_test.wav");
        write_wav(&path, 10);

        let store = Arc::new(AssetStore::new());
        let pool = DecodePool::new(
            1,
            Arc::new(DecodeRegistry::with_builtins()),
            Arc::clone(&store),
        )
        .unwrap();
        pool.submit(AssetId(5), &path);

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while pool.try_poll_completed().is_empty() && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        pool.join();
        assert_eq!(store.get(&AssetId(5)).unwrap().frames(), 10);
    }
}
