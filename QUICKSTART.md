# QUICKSTART — tpt-audio engine library

Build and use the `tpt-av-audio-*` engine workspace. For the archived
router desktop app, see `legacy/` in the repository root.

## Requirements

- Rust 1.75+ (edition 2021)
- Windows: no extra setup (WASAPI backend)
- Linux: PipeWire running with `pw-dump` for device enumeration
  (typically `pipewire` + `wireplumber` packages)
- No audio device needed for offline rendering or tests (`NullBackend`)

## Build & test

```bash
cargo build --all
cargo test --all
```

The real-time safety audit runs as part of `cargo test`
(`tpt-av-audio-core/tests/rt_safety.rs`) and fails the build if the audio
path ever allocates.

## Use as a library

The easy path: one dependency, one struct.

```toml
[dependencies]
tpt-av-audio = "0.1"
```

```rust
use tpt_av_audio::{Engine, Session};

let mut engine = Engine::new(Session::new("demo", 48_000), 512, 2)?;
let voice = engine.load_asset("voice.wav")?;      // decode + cache + register

let mut session = engine.session();
let track = session.add_track("vocals");
let clip = tpt_av_audio::Clip::new(session.generate_clip_id(), voice, 0, 48_000);
session.track_mut(track).unwrap().insert_clip(clip);
engine.set_session(session);

let mut out = tpt_av_audio::AudioBuffer::new(512, 2);
engine.render(&mut out)?;                          // 512 mixed frames, RT-safe
```

One-call helpers: `tpt_av_audio::play_file("song.wav")` (live playback) and
`tpt_av_audio::offline::render_session_to_wav(&session, "mix.wav")`.

Need full control? Add the individual crates — every layer is independently
usable (published names match the directories):

```toml
[dependencies]
tpt-av-audio-utils = "0.1"
tpt-av-audio-timeline = "0.1"
tpt-av-audio-core = "0.1"
tpt-av-audio-io = "0.1"
tpt-av-audio-plugin = "0.1"
```

## Headless rendering (timeline JSON → WAV)

```bash
cargo run -p tpt-av-audio-core --example headless_render -- \
    tpt-av-audio-core/examples/podcast_demo.json demo.wav
```

The JSON is a serialized `tpt_av_audio_timeline::Session`. Asset paths
resolve relative to the JSON file.

## Live playback

```bash
# Play a WAV through the timeline engine:
cargo run -p tpt-av-audio-core --example simple_player -- demo.wav

# Synthesized two-track mix, no file assets needed:
cargo run -p tpt-av-audio-core --example mixer_demo
```

Force the device-free sink (CI, headless machines) with
`TPT_AUDIO_BACKEND=null`.

## Decoding with tpt-cadence

Audio decoding goes through the [`tpt-cadence`](https://github.com/tpt-solutions/tpt-cadence)
codec suite. It is wired in behind a feature with path dependencies on a
sibling checkout (`../tpt-cadence`):

```bash
cargo test  --all --features tpt-av-audio/cadence   # WAV + AIFF + FLAC via cadence
cargo test  --all                                   # built-in WAV-only fallback, no sibling needed
```

Once cadence is pushed to GitHub, swap the path dependencies in the root
`Cargo.toml` for git dependencies and make `cadence` a default feature.

## License

Dual-licensed MIT OR Apache-2.0. Permissive-only dependency policy,
enforced by `cargo-deny` in CI (`deny.toml`).
