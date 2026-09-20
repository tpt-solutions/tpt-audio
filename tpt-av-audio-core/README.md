# tpt-av-audio-core

[![Crates.io](https://img.shields.io/crates/v/tpt-av-audio-core.svg)](https://crates.io/crates/tpt-av-audio-core)
[![docs.rs](https://docs.rs/tpt-av-audio-core/badge.svg)](https://docs.rs/tpt-av-audio-core)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

The **real-time engine** of the [`tpt-av-audio-*`](https://github.com/tpt-solutions/tpt-audio)
workspace: mixer graph, built-in DSP, timeline rendering, asset
management, and lock-free Main-Thread → Audio-Thread state
synchronization.

## Real-time contract

Everything reachable from [`AudioNode::process`] or
[`TimelineRenderer::render`] is **allocation-free, lock-free, and
panic-free** — enforced by design and by an allocation-counting test
(`tests/rt_safety.rs`) plus the shared
[`tpt-av-test`](https://github.com/tpt-solutions/tpt-av-test) harness
(`tests/real_time_harness.rs`). All setup, allocation, and blocking work
happens on the Main Thread through `prepare`/`update` entry points. The
crate's only `unsafe` is the SPSC ring buffer (`ring.rs`), a documented
`UnsafeCell` producer/consumer layout.

## Layer map

| Module | Provides |
| :--- | :--- |
| [`graph`] | [`AudioNode`] trait and the topologically-ordered, summing [`AudioGraph`]. |
| [`mixer`] | Multi-track [`TrackMixer`] — balance-law pan, mute, and solo. |
| [`renderer`] | [`TimelineRenderer`]: snapshot → decoded PCM → clip envelopes/fades → track strip state → mix, with inline sample-rate conversion and playhead tracking. |
| [`scheduler`] | [`TimelineState`] — `arc-swap`-backed, wait-free Main→Audio thread snapshot sync. |
| [`dsp`] | Gain, constant-power pan, position-tracking fades, channel mapping, and resampling (`rubato` sinc offline + allocation-free linear inline). |
| [`overview`] | [`WaveformOverview`] — peak-bucket caches for editor waveform rendering. |
| [`asset`] | [`AssetStore`] — pre-allocated PCM caches per `AudioAsset`, writer-serialized against lost updates, lock-free for readers. |
| [`decode`] | [`DecodeRegistry`] — per-extension decoder dispatch. Built-in WAV decoding needs no external dependency; enable the `cadence` feature for real-time-safe WAV/AIFF/FLAC via [`tpt-cadence`](https://github.com/tpt-solutions/tpt-cadence). |
| [`pool`] | [`DecodePool`] — background worker threads that decode assets off the Main Thread and report completion. |
| [`ring`] | [`ring::SpscRing`] — the lock-free single-producer/single-consumer handoff used by the decode pool. |

## Feature flags

- `cadence` — decode WAV, AIFF, and FLAC through the
  [`tpt-cadence`](https://github.com/tpt-solutions/tpt-cadence) codec
  suite's real-time-safe `Decoder` contract (git dependency, dual
  `MIT OR Apache-2.0`). Without it, `.wav` still decodes via the built-in,
  dependency-free fallback in [`decode`], so a fresh clone builds and
  tests without the sibling checkout.

## Usage

```bash
cargo add tpt-av-audio-core
```

```rust,no_run
use std::sync::Arc;
use tpt_av_audio_core::{AssetStore, DecodeRegistry, TimelineRenderer, TimelineState};
use tpt_av_audio_timeline::Session;
use tpt_av_audio_utils::AudioBuffer;

let session = Session::new("demo", 48_000);
let state = Arc::new(TimelineState::new(session));
let store = Arc::new(AssetStore::new());
let mut renderer = TimelineRenderer::new(state, store);
renderer.prepare(512, 2);

let mut buffer = AudioBuffer::new(512, 2);
renderer.render(&mut buffer)?; // allocation-free, real-time safe
# Ok::<(), tpt_av_audio_utils::AudioError>(())
```

Most applications don't need this crate directly — see the
[`tpt-av-audio`](../tpt-av-audio) facade for a one-call `Engine`, or the
`headless_render`/`simple_player` examples in this crate's `examples/`
directory for a lower-level tour.

## Part of tpt-audio

This is the engine layer of the
[`tpt-audio`](https://github.com/tpt-solutions/tpt-audio) workspace, built
on [`tpt-av-audio-timeline`](../tpt-av-audio-timeline)'s data model and
consumed by [`tpt-av-audio-io`](../tpt-av-audio-io) for OS playback.

## License

Dual-licensed under [MIT](../LICENSE-MIT) OR [Apache-2.0](../LICENSE-APACHE).
