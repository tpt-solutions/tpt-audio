# tpt-audio

**A memory-safe, real-time, non-destructive audio processing engine with hardware I/O. The brain of the TPT AV audio stack.**

**License:** Dual MIT / Apache-2.0 (TPT Solutions) — this overrides spec2's "pure MIT" wording; both licenses apply.
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

Core tenets:

1. **Strictly non-destructive** — source files are never mutated; clips are
   metadata references (trim, split, fade, automate) over `AudioAsset`s.
2. **Real-time safe** — the audio path is allocation-free, lock-free, and
   panic-free, enforced by an allocation-counting test
   (`tpt-av-audio-core/tests/rt_safety.rs`).
3. **Clean thread boundaries** — Main Thread mutates state and decodes;
   Audio Thread only reads lock-free snapshots and mixes.
4. **Permissive-only dependencies** — enforced by `cargo-deny` in CI
   (`deny.toml` denies GPL/LGPL/AGPL/MPL).
5. **Composable crates** — use the timeline alone, just the mixer, or the
   whole engine.

## Ecosystem

```text
tpt-cadence (decodes audio files → raw PCM f32)        [external, pending]
        ↓
tpt-av-audio-timeline (non-destructive edit state)
        ↓
tpt-av-audio-core (real-time mixer graph, envelopes, effects)
        ↓
tpt-av-audio-io (routes the final buffer to OS audio hardware)
```

| Crate | Role |
| :--- | :--- |
| [`tpt-av-audio-utils`](tpt-av-audio-utils) | Shared types: samples, interleaved `AudioBuffer`, time math, `AudioError` |
| [`tpt-av-audio-timeline`](tpt-av-audio-timeline) | Pure data model: `Session`/`Track`/`Clip`/`Envelope`, undoable edits, JSON (de)serialization |
| [`tpt-av-audio-core`](tpt-av-audio-core) | Real-time engine: `AudioGraph`, `TrackMixer`, `TimelineRenderer`, DSP, lock-free `TimelineState`, asset caches, WAV decoding |
| [`tpt-av-audio-io`](tpt-av-audio-io) | OS I/O: device enumeration + streams; WASAPI (Windows), PipeWire (Linux), CoreAudio (stub), Archon (research) |
| [`tpt-av-audio-plugin`](tpt-av-audio-plugin) | Hosting foundation: parameters, envelope-driven automation, buses, side-chaining (CLAP/VST3 hosts are future work) |

## Quickstart

See [QUICKSTART.md](QUICKSTART.md) for build instructions, library usage,
and the headless renderer. The 30-second version:

```bash
cargo build --release
cargo test --all

# Render the bundled demo timeline to a WAV (offline, no audio device):
cargo run -p tpt-av-audio-core --example headless_render -- \
    tpt-av-audio-core/examples/podcast_demo.json demo.wav

# Play a WAV through the timeline engine (uses the live OS backend):
cargo run -p tpt-av-audio-core --example simple_player -- demo.wav
```

## License

Dual-licensed under [MIT](LICENSE-MIT) OR [Apache-2.0](LICENSE-APACHE).
Contributions are accepted under the same terms and must not introduce
copyleft (GPL/LGPL/AGPL/MPL) dependencies.
