# Changelog — tpt-av-audio-utils

All notable changes to this crate are documented here. See the
[workspace CHANGELOG](../CHANGELOG.md) for the full cross-crate history
and the versioning scheme (all crates in this workspace share one version
number).

## [0.2.0] — Unreleased

### Added
- Initial release as part of the `tpt-av-audio-*` engine workspace pivot:
  `Sample` conversions (`f32`/`i16`/`u8`/`i32`) with `convert_to_f32`/
  `convert_from_f32` slice helpers, the canonical interleaved
  `AudioBuffer` (`mix`, `apply_gain`, per-channel read/write,
  `peak`/`rms`/`combined_peak` metering), frame/second/millisecond time
  math with saturating arithmetic, and the shared `AudioError` enum.
- `wav`: a minimal, dependency-free RIFF/WAVE reader and writer (8/16/24/
  32-bit integer PCM, 32-bit IEEE float), added to replace `hound` — see
  the workspace CHANGELOG for why.
