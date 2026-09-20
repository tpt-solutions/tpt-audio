# Changelog — tpt-av-audio-core

All notable changes to this crate are documented here. See the
[workspace CHANGELOG](../CHANGELOG.md) for the full cross-crate history
and the versioning scheme (all crates in this workspace share one version
number).

## [0.2.0] — Unreleased

### Added
- Initial release as part of the `tpt-av-audio-*` engine workspace pivot:
  `AudioGraph`/`AudioNode` (topologically ordered, real-time safe),
  `TrackMixer` (balance-law pan, mute, solo), built-in DSP (gain,
  constant-power pan, position-tracking fade, `rubato` sinc + inline
  linear resampling), `TimelineRenderer` (lock-free snapshot → PCM →
  envelopes/fades → mix, with inline sample-rate conversion), `arc-swap`
  backed `TimelineState` for wait-free Main→Audio thread sync.
- Asset management: `AssetStore` (pre-allocated PCM caches; writer-side
  serialization fixes a lost-update race between concurrent inserts),
  background `DecodePool`, lock-free `SpscRing` handoff.
- `tpt-cadence` integration behind the `cadence` feature: decodes WAV,
  AIFF, and FLAC through cadence's real-time-safe `Decoder` contract via
  `DecodeRegistry`.
- Real-time safety audit: `tests/rt_safety.rs` counts heap allocations
  through a global allocator and asserts the render path is
  allocation-free.
- Examples: `headless_render` (timeline JSON → WAV), `simple_player`
  (WAV → live playback), `mixer_demo` (synthesized multi-track mix).
- Fixed: overlapping clips on the same track overwrote each other in the
  renderer instead of summing — clips now render into a clip-sized
  scratch buffer and are added into the track buffer, so fades and
  crossfades blend the overlap correctly.

### Changed
- Dropped `hound` (Apache-2.0-only, no MIT alternative) in favor of a
  small in-house WAV decoder built on `tpt-av-audio-utils::wav`, used as
  the no-`cadence` fallback.

### Fixed
- 24-bit WAV decoding (the no-`cadence` fallback path) scaled samples by
  2^31 instead of 2^23, quantizing 24-bit audio down to about 1/256th of
  full scale. Found and fixed while replacing `hound`, with a new
  regression test in `decode`.
