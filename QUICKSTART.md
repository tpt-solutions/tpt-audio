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

Add the crates you need to your `Cargo.toml` (published names match the
directory names):

```toml
[dependencies]
tpt-av-audio-utils = "0.1"
tpt-av-audio-timeline = "0.1"
tpt-av-audio-core = "0.1"
tpt-av-audio-io = "0.1"
```

Minimal non-destructive playback of a WAV:

```rust
use std::sync::Arc;
use tpt_av_audio_core::{AssetPcm, AssetStore, TimelineRenderer, TimelineState};
use tpt_av_audio_timeline::{Clip, Session};

let mut session = Session::new("demo", 48_000);
let track = session.add_track("clip");
let asset_id = session.register_asset(/* AudioAsset { … } */);

// Cache decoded PCM (Main Thread), then render (Audio Thread):
let store = Arc::new(AssetStore::new());
store.insert(asset_id, AssetPcm { sample_rate: 48_000, channels: 2, data: pcm });

let state = Arc::new(TimelineState::new(session));
let mut renderer = TimelineRenderer::new(Arc::clone(&state), Arc::clone(&store));
renderer.prepare(256, 2);

let mut buffer = tpt_av_audio_utils::AudioBuffer::new(256, 2);
renderer.render(&mut buffer).unwrap(); // advance the playhead 256 frames
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

## License

Dual-licensed MIT OR Apache-2.0. Permissive-only dependency policy,
enforced by `cargo-deny` in CI (`deny.toml`).
