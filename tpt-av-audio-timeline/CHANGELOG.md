# Changelog — tpt-av-audio-timeline

All notable changes to this crate are documented here. See the
[workspace CHANGELOG](../CHANGELOG.md) for the full cross-crate history
and the versioning scheme (all crates in this workspace share one version
number).

## [0.2.0] — Unreleased

### Added
- Initial release as part of the `tpt-av-audio-*` engine workspace pivot:
  the pure non-destructive data model — `Session`/`Track`/`Clip`/
  `AudioAsset`, `Envelope` with linear/cubic/step interpolation, undoable
  edit operations (insert/remove/move/split with fade continuation and
  envelope continuity across splits), bounded undo/redo `History`, and
  full `serde` support for JSON sessions (`Session::save`/`load`).
- `Clip::new` plus builder methods (`with_fades`, `with_volume_envelope`, …).
- Crossfades: `FadeCurve` (linear/equal-power) per-clip fade, and an
  undoable `CrossfadeEdit` that pulls the right clip over the left tail
  and sets matching equal-power fades.
- Musical time (`musical` module): beat/bar frame math from session
  tempo metadata, `bar_and_beat`/`frame_at_bar_beat` conversions, and
  `snap_to_grid` (bar/beat/half/quarter divisions).
