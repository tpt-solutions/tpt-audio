# Changelog — tpt-av-audio

All notable changes to this crate are documented here. See the
[workspace CHANGELOG](../CHANGELOG.md) for the full cross-crate history
and the versioning scheme (all crates in this workspace share one version
number).

## [0.2.0] — Unreleased

### Added
- Initial release: the one-crate facade over the `tpt-av-audio-*`
  workspace — the `Engine` (timeline state + asset store + decoder
  registry + renderer pre-wired, with blocking and background asset
  loading), `play_file`/`play_file_blocking` one-call playback, and
  `offline::render_session_to_wav`/`render_session_to_wav_opts` (16-bit
  PCM default, 24-bit or 32-bit float via `WavExportFormat`).
- `Engine::apply_edit`/`undo`/`redo`/`can_undo`/`can_redo`
  (`History`-backed, session auto-republished) for undoable edits
  including crossfades.
- `clap` feature: adapts `tpt-av-audio-plugin`'s `HostedPlugin` backends
  onto the engine's `AudioNode` graph via `plugin_bridge::HostedPluginAdapter`.
- `cadence` feature: forwards to `tpt-av-audio-core/cadence` for
  WAV/AIFF/FLAC decoding through the `tpt-cadence` codec suite.

### Changed
- Dropped `hound` (Apache-2.0-only, no MIT alternative) from the offline
  WAV render path in favor of `tpt-av-audio-utils::wav`.
