# tpt-audio — Quickstart

A simple, modern audio router & virtual mixer for Windows. Route any app or
input to any output with a visual matrix and per-app / per-route volume control.

> Status: Windows MVP. Linux (PipeWire) and Archon backends are planned (see
> `todo.md`). The app must be run on Windows with an audio device available.

## Build from source

Prerequisites:

- Rust 1.80+ (install via [rustup](https://rustup.rs))
- Windows 10/11 with a working audio device

```powershell
git clone <repo-url> tpt-audio
cd tpt-audio
cargo build --release -p tpt-audio-desktop
```

The binary is produced at `target/release/tpt-audio-desktop.exe`.

## Run

```powershell
cargo run -p tpt-audio-desktop
```

The app opens an `eframe`/`egui` window with a toolbar of tabs:
**Routing · Volume · Presets · Settings · Diagnostics**.

## Routing tab

- **Rows** are *sources*: microphones/inputs and running apps (`app::<pid>`).
- **Columns** are *sinks*: output devices (speakers, headsets).
- Click **+** at a row/column intersection to create a route.
- Each route has a **mute** checkbox, a **gain** slider (0–100%), and an **x**
  to remove it.
- Devices are auto-detected and refreshed periodically (~every 6 seconds), and
  the matrix updates live.

### Device disconnect / reconnect

Routes survive device unplug events. If an output or input disappears, the
affected routes are marked disconnected and audio for them is paused. When the
device returns (same Windows device id), the pipeline **automatically
reconnects** and resumes — no restart needed. Reconnect activity is visible on
the **Diagnostics** tab (`Device reconnects`).

## Volume tab

- **Master Volume** scales all routed audio.
- **Per-App Volumes** controls each detected app session independently and is
  applied directly via the Windows audio session API.

## Presets tab

- Name the current routing + volume setup and **Save Current**.
- **Load** restores a preset; **Delete** removes it.

> Presets are currently held in-memory. File-based, shareable presets are a
> planned post-1.0 feature (see `todo.md`).

## Settings tab

- Short description of sources/sinks and the auto-refresh behavior.
- **Refresh Devices Now** forces an immediate re-scan.

## Diagnostics tab

Live counters and an event log:

- Stream underruns / overruns and capture glitches (data discontinuity).
- Routes created / removed.
- Devices found / lost / **reconnected**.
- Time since last device refresh.
- Rolling event log with `INFO` / `WARN` / `ERROR` levels.

Use this tab to confirm low-latency, glitch-free operation and to verify
reconnect behavior after unplugging/replugging a device.

## Latency & CPU notes

- Shared-mode WASAPI, 48 kHz / stereo, 256-frame buffers (~5.3 ms per buffer).
- The mixing pipeline pulls capture data as soon as it is available and only
  idles briefly when no audio is flowing, keeping added latency and CPU usage
  low. Capture glitches are surfaced on the Diagnostics tab for tuning.

## Linux (PipeWire)

On Linux the same GUI runs against a PipeWire backend (`platform-linux`). It
talks to a running PipeWire session through the standard CLI tools
(`pw-dump`, `pw-link`, `wpctl`), which are normally provided by the `pipewire`
and `wireplumber` packages.

Requirements:

- PipeWire running with `pw-link` and `wpctl` available on `PATH`.
- A session bus / WirePlumber for `wpctl` volume control.

Behavior differences from Windows:

- **Routing** is performed natively by PipeWire: creating a route makes a graph
  link between the source and sink node ids, so audio stays inside the server
  (zero extra copy). Mute toggles the link.
- **Per-app volume** is applied via `wpctl set-volume`.
- **Per-route gain** is recorded by the UI but not yet applied by the link
  backend (a PipeWire link is unity-gain). A future native `pipewire-rs` backend
  can apply per-route gain through a mixing proxy.
- No mixing "stream" is started; links are live as soon as a route exists.

## Archon (research-gated)

Archon support is scaffolded in `platform-archon` behind the same `AudioBackend`
trait, including the `AUDIO_CAPTURE` capability request/grant model
(`Capability`, `CapabilityGrant`, `request_audio_capture()`). The actual
transport depends on the `tpt-archon` audio server API and `tpt-archon-bridge`
zero-copy IPC, which are not yet published, so the backend currently returns
research-gated errors. Build with `cargo run -p tpt-audio-desktop --features
archon` on an Archon target to exercise the scaffold.
