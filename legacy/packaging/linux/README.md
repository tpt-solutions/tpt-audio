# Linux packaging for tpt-audio

tpt-audio targets **PipeWire** on Linux. The routing matrix GUI is identical across
platforms; only the backend (`platform-linux`) differs.

## Flatpak (recommended)

The manifest lives at `flatpak/io.github.tptsolutions.tptaudio.yaml`. To build and test
locally:

```bash
# from the repository root
flatpak-builder build --force-clean \
  packaging/linux/flatpak/io.github.tptsolutions.tptaudio.yaml

flatpak-builder --run build io.github.tptsolutions.tptaudio
```

To publish on Flathub, push the manifest (and metainfo/desktop files) to a Flathub repo
and submit a PR. The `org.freedesktop.Platform` runtime provides PipeWire client libs;
`tpt-audio` talks to PipeWire through the standard CLI tools (`pw-link`, `wpctl`,
`pw-dump`), so no extra PipeWire build dependency is required.

## .deb (Debian/Ubuntu)

1. Build the release binary: `cargo build --release` (produces `target/release/tpt-audio-desktop`).
2. Lay out a package tree:
   ```
   deb/
     usr/bin/tpt-audio-desktop
     usr/share/applications/io.github.tptsolutions.tptaudio.desktop
     usr/share/metainfo/io.github.tptsolutions.tptaudio.metainfo.xml
     DEBIAN/control
   ```
3. `dpkg-deb --build deb tpt-audio_<ver>_amd64.deb`.

A `DEBIAN/control` should declare `Depends: pipewire, libpipewire-0.3-0, libgtk-3-0`
and `Recommends: wireplumber`.

## AppImage

Use `linuxdeploy` with the PipeWire/`pw-*` tools bundled, or rely on the host PipeWire
tooling. GStreamer/GTK runtime is pulled from the host. Mark the bundle as requiring
PipeWire.

## Notes

- The Linux backend requires `pw-dump`, `pw-link`, and `wpctl` on `PATH`.
- Per-app volume uses `wpctl set-volume` per stream node.
- A future native `pipewire-rs` backend can apply per-route gain via a mixing proxy.
