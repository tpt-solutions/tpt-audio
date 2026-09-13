//! Lock-free bounded SPSC ring buffer for decoder → audio-thread handoff.
//!
//! One producer (a decode worker) pushes interleaved f32; one consumer (the
//! audio thread) pops. Fixed capacity, power-of-two, preallocated: `push`/
//! `pop` are wait-free atomic index shuffles with no allocation and no lock.
//!
//! Capacity protocol: `push_slice` writes as much as fits and returns the
//! number of frames written — the producer never blocks (it drops/defers
//! excess, e.g. by re-reading the source later); `pop_slice` fills what is
//! available and returns the frames read — the consumer pads with silence.

use std::cell::UnsafeCell;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Bounded single-producer/single-consumer f32 ring (units: **frames**,
/// interleaved across `channels`).
///
/// The data slot is an `UnsafeCell`; this is the canonical SPSC layout: the
/// producer is the only writer, the consumer the only reader, and the
/// acquire/release index protocol guarantees the reader never observes a
/// partially-written run. `unsafe impl Sync` is what makes sharing across
/// the two threads sound *under that discipline* — one producer, one
/// consumer, nothing else touches the ring.
pub struct SpscRing {
    buf: UnsafeCell<Box<[f32]>>,
    channels: usize,
    capacity_frames: usize, // power of two
    mask: usize,            // capacity_frames * channels - 1 (sample mask)
    read: AtomicUsize,      // sample index
    write: AtomicUsize,     // sample index
}

unsafe impl Sync for SpscRing {}

impl SpscRing {
    /// Creates a ring holding exactly `capacity_frames` frames of
    /// `channels` interleaved channels. Capacity is rounded up to a power
    /// of two; [`Self::capacity_frames`] reports the actual value.
    pub fn new(capacity_frames: usize, channels: u16) -> Self {
        let channels = channels.max(1) as usize;
        let cap = capacity_frames.max(1).next_power_of_two();
        Self {
            buf: UnsafeCell::new(vec![0.0; cap * channels].into_boxed_slice()),
            channels,
            capacity_frames: cap,
            mask: cap * channels - 1,
            read: AtomicUsize::new(0),
            write: AtomicUsize::new(0),
        }
    }

    /// Usable capacity in frames (power of two).
    pub fn capacity_frames(&self) -> usize {
        self.capacity_frames
    }

    fn buf_len(&self) -> usize {
        // SAFETY: shared read of the slice length; no mutation involved.
        unsafe { (&*self.buf.get()).len() }
    }

    /// Channel count.
    pub fn channels(&self) -> u16 {
        self.channels as u16
    }

    /// Frames currently available to the consumer.
    pub fn len(&self) -> usize {
        let write = self.write.load(Ordering::Acquire);
        let read = self.read.load(Ordering::Acquire);
        (write.wrapping_sub(read)) / self.channels
    }

    /// Whether the ring holds no readable frames.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Free frames available to the producer.
    pub fn free_frames(&self) -> usize {
        self.capacity_frames - self.len()
    }

    /// Pushes `frames` interleaved frames from `data` (which may hold up to
    /// `frames * channels` samples). Writes as many frames as fit; returns
    /// the frames written. Producer side only.
    pub fn push_slice(&self, data: &[f32]) -> usize {
        let channels = self.channels;
        let frames = data.len() / channels;
        let mut written = 0usize;

        while written < frames {
            let write = self.write.load(Ordering::Acquire);
            let read = self.read.load(Ordering::Acquire);
            let used = write.wrapping_sub(read);
            let free_samples = self.buf_len() - used;
            if free_samples == 0 {
                break;
            }
            // Contiguous run until either the data ends or the slot wraps.
            let run_samples = free_samples
                .min((frames - written) * channels)
                .min(self.buf_len() - (write & self.mask));
            let src = &data[written * channels..written * channels + run_samples];
            let start = write & self.mask;
            // SAFETY: producer is the unique writer; the release-store on
            // `write` publishes the run to the consumer after it lands.
            let buf = unsafe { &mut *self.buf.get() };
            buf[start..start + run_samples].copy_from_slice(src);
            self.write
                .store(write.wrapping_add(run_samples), Ordering::Release);
            written += run_samples / channels;
        }
        written
    }

    /// Pops up to `frames` frames into `dst` at `dst_offset_frames`,
    /// interleaved. Returns frames read. Consumer side only.
    ///
    /// # Real-Time Safety
    ///
    /// Wait-free: bounded by capacity, no allocation, no lock.
    pub fn pop_slice(&self, dst: &mut [f32], dst_offset_frames: usize, frames: usize) -> usize {
        let channels = self.channels;
        let mut popped = 0usize;

        while popped < frames {
            let write = self.write.load(Ordering::Acquire);
            let read = self.read.load(Ordering::Acquire);
            let used = write.wrapping_sub(read);
            if used == 0 {
                break;
            }
            let run_samples = used
                .min((frames - popped) * channels)
                .min(self.buf_len() - (read & self.mask));
            let start = read & self.mask;
            // SAFETY: consumer is the unique reader; the acquire-load of
            // `write` above ordered the producer's writes before this read.
            let buf = unsafe { &*self.buf.get() };
            dst.copy_from_slice_within(
                &buf[start..start + run_samples],
                (dst_offset_frames + popped) * channels,
            );
            self.read
                .store(read.wrapping_add(run_samples), Ordering::Release);
            popped += run_samples / channels;
        }
        popped
    }
}

/// Helper trait local to this module so `pop_slice` can copy into a caller
/// offset without slicing panics.
trait CopyWithin {
    fn copy_from_slice_within(&mut self, src: &[f32], offset: usize);
}

impl CopyWithin for [f32] {
    fn copy_from_slice_within(&mut self, src: &[f32], offset: usize) {
        let end = (offset + src.len()).min(self.len());
        if offset < end {
            self[offset..end].copy_from_slice(&src[..end - offset]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_pop_round_trip() {
        let ring = SpscRing::new(8, 2);
        let data: Vec<f32> = (0..16).map(|i| i as f32).collect();
        let written = ring.push_slice(&data);
        assert_eq!(written, 8);
        assert_eq!(ring.len(), 8);

        let mut dst = vec![0.0f32; 16];
        let read = ring.pop_slice(&mut dst, 0, 8);
        assert_eq!(read, 8);
        assert_eq!(dst, data);
        assert!(ring.is_empty());
    }

    #[test]
    fn wraps_around_seamlessly() {
        let ring = SpscRing::new(4, 1);
        let mut dst = Vec::new();

        for round in 0..3u32 {
            let written = ring.push_slice(&[round as f32; 3]);
            assert_eq!(written, 3);
            let mut out = [0.0f32; 3];
            assert_eq!(ring.pop_slice(&mut out, 0, 3), 3);
            assert_eq!(out, [round as f32; 3]);
            dst.push(round);
        }
        assert_eq!(dst, [0, 1, 2]);
    }

    #[test]
    fn capacity_is_power_of_two() {
        let ring = SpscRing::new(10, 2);
        assert_eq!(ring.capacity_frames(), 16);
    }

    #[test]
    fn push_past_capacity_writes_partial() {
        let ring = SpscRing::new(4, 1);
        let data: Vec<f32> = (0..10).map(|i| i as f32).collect();
        let written = ring.push_slice(&data);
        assert_eq!(written, 4);
        assert_eq!(ring.free_frames(), 0);

        let mut out = [0.0f32; 4];
        assert_eq!(ring.pop_slice(&mut out, 0, 4), 4);
        assert_eq!(out, [0.0, 1.0, 2.0, 3.0]);
    }

    #[test]
    fn pop_from_empty_returns_zero() {
        let ring = SpscRing::new(4, 2);
        let mut out = [7.0f32; 4];
        assert_eq!(ring.pop_slice(&mut out, 0, 2), 0);
        assert_eq!(out, [7.0; 4]); // untouched — caller pads silence itself
    }

    #[test]
    fn concurrent_producer_consumer_no_loss_under_load() {
        let ring = std::sync::Arc::new(SpscRing::new(1024, 2));
        const TOTAL: usize = 200_000;

        let producer = {
            let ring = std::sync::Arc::clone(&ring);
            std::thread::spawn(move || {
                let mut sent = 0usize;
                let mut value = 0.0f32;
                while sent < TOTAL {
                    let chunk = 64;
                    let data: Vec<f32> = (0..chunk * 2).map(|_| value).collect();
                    let written = ring.push_slice(&data);
                    sent += written;
                    value += 1.0;
                    if written == 0 {
                        std::thread::yield_now();
                    }
                }
            })
        };

        let consumer = {
            let ring = std::sync::Arc::clone(&ring);
            std::thread::spawn(move || {
                let mut received = 0usize;
                let mut dst = vec![0.0f32; 64 * 2];
                while received < TOTAL {
                    let got = ring.pop_slice(&mut dst, 0, 64);
                    received += got;
                    if got == 0 {
                        std::thread::yield_now();
                    }
                }
                received
            })
        };

        producer.join().unwrap();
        let received = consumer.join().unwrap();
        assert_eq!(received, TOTAL);
    }
}
