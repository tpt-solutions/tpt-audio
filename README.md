# tpt-audio

[![CI](https://github.com/tpt-solutions/tpt-audio/actions/workflows/ci.yml/badge.svg)](https://github.com/tpt-solutions/tpt-audio/actions/workflows/ci.yml)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)
[![Rust 1.75+](https://img.shields.io/badge/rust-1.75%2B-orange.svg)](https://www.rust-lang.org)

**A memory-safe, real-time, non-destructive audio processing engine with hardware I/O. The brain of the TPT AV audio stack.**

**Status:** Early-stage / Pre-1.0
**Ecosystem:** [TPT Solutions Open Source](https://opensource.tptsolutions.co.nz/)

---

> **Pivot note:** this repository previously housed a *router/mixer desktop
> app* (per-app audio routing with `core`/`gui`/`desktop`/`platform-*`
> crates). It has pivoted into the `tpt-av-audio-*` **engine library
> workspace** described in `spec2.txt`. The complete old app is preserved,
> buildable and unmodified, under [`legacy/`](legacy/) (along with the old
> `spec.txt`, installer manifests, and app-distribution docs).

---

## What it is

`tpt-audio` is the **audio processing layer** of the TPT AV Stack. It
provides the foundational crates for building non-destructive audio
editors, DAWs, podcast tools, and game-audio middleware on top of raw PCM
from [`tpt-cadence`](https://github.com/tpt-solutions/tpt-cadence).

Decoding runs on the [`tpt-cadence`](https://github.com/tpt-solutions/tpt-cadence)
codec suite (pure Rust, MIT OR Apache-2.0): build with
`--features tpt-av-audio/cadence` to enable WAV/AIFF/FLAC via cadence's
real-time-safe decoders. Without the feature (or before cadence is pushed to
GitHub — it is currently a sibling checkout wired through path dependencies),
WAV decoding falls back to hound so CI and fresh clones still build.

Core tenets:

1. **Strictly non-destructive** — source files are never mutated; clips are
   metadata references (trim, split, fade, automate) over `AudioAsset`s.
2. **Real-time safe** — the audio path is allocation-free, lock-free, and
   panic-free, enforced by an allocation-counting test
   (`tpt-av-audio-core/tests/rt_safety.rs`).
3. **Clean thread boundaries** — Main Thread mutates state and decodes;
   Audio Thread only reads lock-free snapshots and mixes.
4. **Permissive-only, audited dependencies** — `cargo-deny` in CI denies
   GPL/LGPL/AGPL/MPL, RustSec advisories, and non-crates.io sources (see
   [SECURITY.md](SECURITY.md)).
5. **Composable crates** — start with the one-crate facade or pick individual layers.

## Ecosystem

```text
tpt-cadence (decodes audio files → raw PCM f32)        [external]
        ↓
tpt-av-audio-timeline (non-destructive edit state)
        ↓
tpt-av-audio-core (real-time mixer graph, envelopes, effects)
        ↓
tpt-av-audio-io (routes the final buffer to OS audio hardware)
```

| Crate | Role |
| :--- | :--- |
| [`tpt-av-audio`](tpt-av-audio) | **Start here** — one-crate facade: `Engine`, one-call playback/offline render |
| [`tpt-av-audio-utils`](tpt-av-audio-utils) | Shared types: samples, interleaved `AudioBuffer` (with peak/RMS meters), time math, `AudioError` |
| [`tpt-av-audio-timeline`](tpt-av-audio-timeline) | Pure data model: `Session`/`Track`/`Clip`/`Envelope`, undoable edits, `Session::save`/`load` JSON documents |
| [`tpt-av-audio-core`](tpt-av-audio-core) | Real-time engine: `AudioGraph`, `TrackMixer`, `TimelineRenderer`, DSP, lock-free `TimelineState`, asset caches, cadence-backed decoding |
| [`tpt-av-audio-io`](tpt-av-audio-io) | OS I/O: device enumeration + streams; WASAPI (Windows), PipeWire (Linux, `pw-dump` + `pw-cat` streams), CoreAudio (stub), Archon (research) |
| [`tpt-av-audio-plugin`](tpt-av-audio-plugin) | Hosting foundation: parameters, envelope-driven automation, buses, side-chaining (CLAP/VST3 hosts are future work) |

## Quickstart

Add the facade:

```bash
cargo add tpt-av-audio
```

```rust
use tpt_av_audio::{Engine, Session};

let mut engine = Engine::new(Session::new("demo", 48_000), 512, 2)?;
let voice = engine.load_asset("voice.wav")?;   // decode + cache + register

let mut session = engine.session();
let track = session.add_track("vocals");
let clip = tpt_av_audio::Clip::new(session.generate_clip_id(), voice, 0, 48_000);
session.track_mut(track).unwrap().insert_clip(clip);
engine.set_session(session);

let mut out = tpt_av_audio::AudioBuffer::new(512, 2);
engine.render(&mut out)?;                      // 512 mixed frames, RT-safe
```

More:

```bash
cargo test --all

# Render the bundled demo timeline to a WAV (offline, no audio device):
cargo run -p tpt-av-audio-core --example headless_render -- \
    tpt-av-audio-core/examples/podcast_demo.json demo.wav

# Play a WAV through the timeline engine (live OS backend):
cargo run -p tpt-av-audio-core --example simple_player -- demo.wav

# The smallest possible programs, using the facade:
cargo run -p tpt-av-audio --example play_file -- demo.wav
cargo run -p tpt-av-audio --example hello_render
```

See [QUICKSTART.md](QUICKSTART.md) for the full walkthrough and
[CONTRIBUTING.md](CONTRIBUTING.md) for the project rules (real-time safety,
dependency policy).

## License

Dual-licensed under [MIT](LICENSE-MIT) OR [Apache-2.0](LICENSE-APACHE).
Contributions are accepted under the same terms and must not introduce
copyleft (GPL/LGPL/AGPL/MPL) dependencies.
