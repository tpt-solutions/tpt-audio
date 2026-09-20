# tpt-av-audio-timeline

[![Crates.io](https://img.shields.io/crates/v/tpt-av-audio-timeline.svg)](https://crates.io/crates/tpt-av-audio-timeline)
[![docs.rs](https://docs.rs/tpt-av-audio-timeline/badge.svg)](https://docs.rs/tpt-av-audio-timeline)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

The pure, non-destructive edit **data model** for the
[`tpt-av-audio-*`](https://github.com/tpt-solutions/tpt-audio) engine
workspace. This crate describes *what* audio plays, *when*, and *how* — it
never touches audio itself (no rendering, no I/O, `#![forbid(unsafe_code)]`).
Source media files are never mutated; everything here is metadata layered
over them.

## Layer map

| Type | Role |
| :--- | :--- |
| [`Session`] | Top-level document: a collection of [`Track`]s, sample rate, and metadata. Round-trips as JSON via `serde` (`Session::save`/`load`). |
| [`Track`] | A named strip of [`Clip`]s with volume, pan, mute, and solo. |
| [`Clip`] | A time range referencing an [`AudioAsset`], with fades ([`FadeCurve`]: linear or equal-power) and per-parameter [`Envelope`]s. |
| [`AudioAsset`] | A file on disk plus its decoded-format facts (sample rate, channels, duration). |
| [`Envelope`] | Automation points with linear, cubic, or step interpolation — used for volume/pan automation and fade shaping. |
| [`edit`] / [`History`] | Undoable operations over a `Session` — insert, remove, move, split, and crossfade clips — with fade continuation and envelope continuity preserved across splits, plus bounded undo/redo. |
| [`musical`] | Beat/bar frame math from session tempo metadata: `bar_and_beat`/`frame_at_bar_beat` conversions and `snap_to_grid`. |

## Usage

```bash
cargo add tpt-av-audio-timeline
```

```rust
use tpt_av_audio_timeline::{Session, Clip, TrackId};

let mut session = Session::new("demo", 48_000);
let asset_id = session.register_asset(tpt_av_audio_timeline::AudioAsset {
    id: tpt_av_audio_timeline::AssetId(0),
    file_path: "voice.wav".into(),
    duration_frames: 48_000,
    sample_rate: 48_000,
    channels: 2,
});

let track = session.add_track("vocals");
let clip_id = session.generate_clip_id();
session
    .track_mut(TrackId(track.0))
    .unwrap()
    .insert_clip(Clip::new(clip_id, asset_id, 0, 48_000));

session.save("session.json")?;
let reloaded = Session::load("session.json")?;
# Ok::<(), tpt_av_audio_utils::AudioError>(())
```

Undoable edits go through [`Edit`]/[`History`] rather than mutating a
`Session` directly, so applications get undo/redo for free:

```rust,ignore
let edit = InsertClipEdit::new(track_id, clip);
history.apply(Box::new(edit), &mut session)?;
history.undo(&mut session)?;
```

## Part of tpt-audio

This crate is one layer of the [`tpt-audio`](https://github.com/tpt-solutions/tpt-audio)
engine. [`tpt-av-audio-core`](../tpt-av-audio-core) renders a `Session`
into real-time audio; the [`tpt-av-audio`](../tpt-av-audio) facade wires
everything together if you just want to get started.

## License

Dual-licensed under [MIT](../LICENSE-MIT) OR [Apache-2.0](../LICENSE-APACHE).
