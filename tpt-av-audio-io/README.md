# tpt-av-audio-io

[![Crates.io](https://img.shields.io/crates/v/tpt-av-audio-io.svg)](https://crates.io/crates/tpt-av-audio-io)
[![docs.rs](https://docs.rs/tpt-av-audio-io/badge.svg)](https://docs.rs/tpt-av-audio-io)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

OS audio I/O for the [`tpt-av-audio-*`](https://github.com/tpt-solutions/tpt-audio)
engine: device enumeration, stream management, and the platform backends
that get a mixed buffer out to real hardware.

## Backends

| Platform | Backend | Status |
| :--- | :--- | :--- |
| Windows | WASAPI | Shared-mode render/capture, `AUTOCONVERTPCM` format adaptation, loopback capture of output devices, lazy per-thread COM init. **Verified live on Windows hardware.** Unsafe code is confined to this backend's COM interop; every call site documents its invariants. |
| Linux | PipeWire | Device enumeration via `pw-dump`; playback/capture via `pw-cat` raw f32 pipes (`PwCatWriter`/`PwCatReader`). A native `pipewire-rs` port is the planned upgrade. Unsafe-free. |
| macOS | CoreAudio | Stub — new surface, not yet implemented. |
| — | Archon | Research-gated capability-broker stub; blocked on an upstream API that isn't public yet. |
| Any | [`NullBackend`] | Dependency-free sink for tests and CI. Force it with `TPT_AUDIO_BACKEND=null`. |

[`backend::default_backend`] picks the right backend for the current
platform at runtime.

## API surface

- [`device::enumerate_devices`] / [`AudioDevice`] — list input/output devices.
- [`stream::OutputStream`] / [`stream::InputStream`] — open a device and push/pull `AudioBuffer`s through a callback.
- [`router::VirtualRouter`] — in-process mixing router (groundwork for a future virtual-device core).
- [`AudioBackend`] — the trait every platform backend implements, if you need to plug in your own.

## Usage

```bash
cargo add tpt-av-audio-io
```

```rust,no_run
use tpt_av_audio_io::{enumerate_devices, Direction};

for device in enumerate_devices()? {
    if device.direction == Direction::Output {
        println!("{}: {}", device.id.0, device.name);
    }
}
# Ok::<(), tpt_av_audio_utils::AudioError>(())
```

For a full render → device pipeline, see the
[`tpt-av-audio`](../tpt-av-audio) facade's `play_file`, or
`tpt-av-audio-core`'s `simple_player` example.

## Testing without hardware

Set `TPT_AUDIO_BACKEND=null` to force [`NullBackend`], or construct one
directly — this is how the crate's own test suite runs in CI without
requiring an audio device.

## Part of tpt-audio

The playback endpoint of the [`tpt-audio`](https://github.com/tpt-solutions/tpt-audio)
workspace: it takes the mixed `AudioBuffer` that
[`tpt-av-audio-core`](../tpt-av-audio-core) renders and gets it to
hardware (or pulls captured audio in the other direction).

## License

Dual-licensed under [MIT](../LICENSE-MIT) OR [Apache-2.0](../LICENSE-APACHE).
