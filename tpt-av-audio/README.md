# tpt-av-audio

[![Crates.io](https://img.shields.io/crates/v/tpt-av-audio.svg)](https://crates.io/crates/tpt-av-audio)
[![docs.rs](https://docs.rs/tpt-av-audio/badge.svg)](https://docs.rs/tpt-av-audio)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

The one-crate facade over the [`tpt-av-audio-*`](https://github.com/tpt-solutions/tpt-audio)
engine workspace. Add this single dependency and get the whole
non-destructive audio engine: timeline, real-time mixer, OS I/O, and
decoding, pre-wired into an [`Engine`].

For the full ecosystem overview, feature-flag reference, and design
tenets, see the [workspace root README](https://github.com/tpt-solutions/tpt-audio#readme).
This crate's README covers the facade itself.

## The 20-second version

```rust,no_run
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
# Ok::<(), tpt_av_audio_utils::AudioError>(())
```

## One-call helpers

- [`offline::render_session_to_wav`] / [`offline::render_session_to_wav_opts`] — render a `Session` straight to a WAV file (16-bit PCM by default; 24-bit or 32-bit float via [`WavExportFormat`]), no audio device needed.
- [`play_file`] / [`play_file_blocking`] — decode a file and play it through the OS backend in one call.
- [`Engine`] — the pre-wired struct for everything else: load assets, apply undoable edits (`apply_edit`/`undo`/`redo`), and render frames or a live stream.

## Feature flags

- `cadence` — forwards to `tpt-av-audio-core/cadence`: decode WAV/AIFF/FLAC through the [`tpt-cadence`](https://github.com/tpt-solutions/tpt-cadence) codec suite instead of the built-in WAV-only fallback.
- `clap` — CLAP plugin hosting: adapts [`tpt-av-audio-plugin`](../tpt-av-audio-plugin)'s `HostedPlugin` backends (via [`plugin_bridge::HostedPluginAdapter`]) onto the engine's `AudioNode` graph.

## Re-exported workspace crates

Full control (custom graphs, DSP nodes, plugin buses, backends) lives in
the underlying crates, re-exported here for one-stop imports:

```rust,ignore
use tpt_av_audio::core_engine; // tpt-av-audio-core
use tpt_av_audio::io;          // tpt-av-audio-io
use tpt_av_audio::timeline;    // tpt-av-audio-timeline
use tpt_av_audio::utils;       // tpt-av-audio-utils
```

## Examples

```bash
cargo run -p tpt-av-audio --example play_file -- demo.wav
cargo run -p tpt-av-audio --example hello_render
```

See the [QUICKSTART](https://github.com/tpt-solutions/tpt-audio/blob/master/QUICKSTART.md)
for the full walkthrough.

## License

Dual-licensed under [MIT](../LICENSE-MIT) OR [Apache-2.0](../LICENSE-APACHE).
