# tpt-av-audio-utils

[![Crates.io](https://img.shields.io/crates/v/tpt-av-audio-utils.svg)](https://crates.io/crates/tpt-av-audio-utils)
[![docs.rs](https://docs.rs/tpt-av-audio-utils/badge.svg)](https://docs.rs/tpt-av-audio-utils)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

The dependency-free foundation layer of the [`tpt-av-audio-*`](https://github.com/tpt-solutions/tpt-audio)
engine workspace: sample conversions, the canonical interleaved audio
buffer, time math, a minimal WAV codec, and the shared error type. Every
other crate in the workspace builds on this one, so it intentionally has
**zero external dependencies** — no licensing or compile-time risk sits
underneath it.

## What's in here

| Module | Provides |
| :--- | :--- |
| [`sample`] | The [`Sample`] trait: lossless-as-possible conversions between the engine's canonical `f32 [-1.0, 1.0]` format and `i16`/`u8`/`i32` PCM widths, plus `convert_to_f32`/`convert_from_f32` slice helpers. |
| [`buffer`] | [`AudioBuffer`] — the canonical interleaved sample buffer used everywhere in the engine, with `mix`, `apply_gain`, per-channel read/write, and `peak`/`rms`/`combined_peak` level metering. |
| [`time`] | Frame/second/[`Milliseconds`] conversions with saturating arithmetic (no panics on the audio path). |
| [`error`] | [`AudioError`] — the one error enum shared across the whole workspace (backend failures, decode errors, invalid edits, missing assets, …). |
| [`wav`] | A minimal RIFF/WAVE reader and writer: 8/16/24/32-bit integer PCM and 32-bit IEEE float, canonical (non-extensible) headers, any channel count. Not a general-purpose WAV library — see [`tpt-cadence`](https://github.com/tpt-solutions/tpt-cadence) (opt-in via the `cadence` feature elsewhere in the workspace) for broader real-world WAV/AIFF/FLAC decoding. |

## Why a WAV codec lives here

This workspace's dependency policy requires every crate to offer an MIT
license alternative (`deny.toml` only allows bare `Apache-2.0` never, only
`MIT OR Apache-2.0` or better). The obvious crates.io WAV library, `hound`,
is Apache-2.0-only, so [`wav`] exists to cover exactly the formats the
engine produces and consumes (16-bit PCM render output, decode of common
WAV files) without adding a dependency at all.

```rust
use tpt_av_audio_utils::wav::{SampleFormat, WavSpec, WavWriter, WavReader};

let spec = WavSpec {
    channels: 2,
    sample_rate: 48_000,
    bits_per_sample: 16,
    sample_format: SampleFormat::Int,
};
let mut writer = WavWriter::create("out.wav", spec)?;
writer.write_sample(16_000i16)?;
writer.write_sample(-16_000i16)?;
writer.finalize()?;

let mut reader = WavReader::open("out.wav")?;
let samples: Vec<i16> = reader.samples::<i16>().collect::<Result<_, _>>()?;
# Ok::<(), tpt_av_audio_utils::AudioError>(())
```

## Usage

```bash
cargo add tpt-av-audio-utils
```

```rust
use tpt_av_audio_utils::{AudioBuffer, Sample};

let mut buffer = AudioBuffer::new(512, 2); // 512 frames, stereo
buffer.apply_gain(0.5);
let peak = buffer.peak();

let sample: i16 = Sample::from_f32(0.75);
```

## Part of tpt-audio

See the [workspace root](https://github.com/tpt-solutions/tpt-audio) for
the full engine (timeline, real-time mixer, OS I/O, plugin hosting) built
on top of this crate.

## License

Dual-licensed under [MIT](../LICENSE-MIT) OR [Apache-2.0](../LICENSE-APACHE).
