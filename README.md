# cptv-vlc

A VLC 3.0 plugin that plays [Cacophony Project](https://cacophony.org.nz) **CPTV** thermal
video files, so you can double-click a `.cptv` file and watch it in VLC on Linux or macOS.

Decoding uses the upstream Rust decoder from
[cptv-tools](https://github.com/TheCacophonyProject/cptv-tools) (`cptv-codec-rs`). VLC plugins
are loaded through a C ABI full of preprocessor macros and version-specific structs, so a small
C file handles the VLC side and the Rust code does the rest.

```
vlc/cptv_demux.c   VLC module descriptor + demux callbacks (open/demux/seek/meta)
vlc/cptv_core.h    C interface to the Rust core
src/lib.rs         probe, open via read/seek callbacks, normalise + upscale + colourise
src/source.rs      frame sources: streaming (decode on demand) or in memory
src/palette.rs     colour maps (viridis, inferno, magma, plasma, turbo, ironbow, grey)
tests/common/      tiny CPTV encoder for synthetic test clips
```

The Rust code reads the file straight from VLC's stream. How it gets frames depends on the
header:

- **Header has `total_frames`** (newer cameras, such as TC2): frames are decoded on demand
  and only the current one is kept. The length and seek bar come from the header. CPTV frames
  are delta-coded inside a single gzip stream, so there's nowhere to jump into the middle.
  Seeking forward decodes up to the target, and seeking backward rewinds the stream and
  decodes from the start. Decoding is fast: for a 10-minute clip (5,400 frames), the worst
  case of seeking back to the start and then to the last frame takes about 160 ms. VLC's
  peak memory for that clip was 85 MB, against 290 MB when the whole clip is held in memory.
- **No `total_frames`** (older files): the length isn't known until the end, so the whole
  clip is decoded into memory when the file opens (about 38 KB per frame), and seeking is
  instant.

Each frame is drawn as an RV32 picture when it's needed and passed to VLC's `rawvideo`
decoder. If there's a background frame, it's skipped. If a header claims more frames than
the file holds (for example, a truncated recording), playback simply ends at the last frame
that decodes.

With clip normalisation (the default), the brightness range comes from the min/max in the
header. If the header doesn't have them, the plugin reads through every frame once when the
file opens to find them, but doesn't keep the frames.

## Build

You need Rust (stable), a C compiler, `git`, and VLC 3.0.x.

```bash
make            # → build/libcptv_plugin.so (Linux) or build/libcptv_plugin.dylib (macOS)
make test       # Rust tests + a headless VLC run that checks frames come out
```

The tests use the sample `.cptv` files from cptv-tools (`cptv-codec-rs/tests/fixtures`),
read straight from Cargo's checkout of that dependency at the commit pinned in `Cargo.toml`.
That way this repo carries no binary samples, and any new upstream samples are tested when
you update the pin. `scripts/fixtures-dir.sh` prints where they are. Long clips are made on
the fly by a small encoder in `tests/common/`.

The first build downloads VLC's public plugin headers (only `include/`, from
code.videolan.org) into `third_party/`. If `libvlccore-dev` is installed, its headers are used
instead.

VLC loads a plugin only if its entry-point name has the exact ABI tag VLC expects. Upstream
builds use `vlc_entry__3_0_0f`. Debian/Ubuntu's 64-bit `time_t` builds use `…3_0_0ft64`. The
Makefile finds the right tag by reading your installed `libvlccore`. To set it yourself, use
`make VLC_ABI=3_0_0f`.

## Install and make double-click work

### Linux, no root (recommended)

```bash
make install-user
```

This command:

- puts the plugin in `~/.local/lib/cptv-vlc/plugins/`
- registers a `video/x-cptv` MIME type for `*.cptv` in `~/.local/share/mime`
- adds a hidden launcher, "VLC (CPTV thermal video)" (`~/.local/share/applications/vlc-cptv.desktop`),
  which runs VLC with `VLC_PLUGIN_PATH` pointing at the plugin
- makes that launcher the default app for `.cptv` files

To undo it, run `make uninstall-user`.

The launcher always starts a new VLC window. It doesn't send the file to a VLC that's already
running, because that copy may not have the plugin loaded.

### Linux, system-wide

```bash
sudo make install     # copies into /usr/lib/<triplet>/vlc/plugins/demux and refreshes VLC's plugin cache
make associate        # .cptv → MIME type, and vlc.desktop as the default app
```

This works for any VLC launch, including the command line and "Open With". It won't work
with Snap or Flatpak builds of VLC, which can't see host plugins. Use the distro package.

### macOS

```bash
make install          # copies the ad-hoc-signed dylib into /Applications/VLC.app/Contents/MacOS/plugins
make associate        # uses `duti` if installed; otherwise:
                      #   Finder → select a .cptv → Get Info → Open with: VLC → Change All…
```

Notes for macOS (untested so far; the build has only been run on Linux):

- Build on the same architecture as your VLC.app (arm64 or x86_64).
  For example, run `rustup target add x86_64-apple-darwin` and set `CARGO_BUILD_TARGET`
  if you're running an Intel VLC on Apple Silicon.
- Copying a file into `VLC.app` changes the signed app bundle. If VLC won't load the plugin
  (check **Window → Messages** with verbosity set to 2), re-sign the app ad hoc:
  `codesign --force --deep --sign - /Applications/VLC.app`.
- If you'd rather not change the app bundle, run
  `VLC_PLUGIN_PATH=$PWD/build /Applications/VLC.app/Contents/MacOS/VLC file.cptv`.
- To use a VLC.app somewhere else, set `VLC_APP=/path/to/VLC.app`.

## Options

You can change these in **Tools → Preferences → Show settings: All → Input / Codecs → Demuxers → CPTV**,
or pass them on the command line:

| Option | Default | Description |
|---|---|---|
| `--cptv-palette` | `viridis` | `viridis`, `inferno`, `magma`, `plasma`, `turbo`, `ironbow`, `grey` |
| `--cptv-normalize-clip` | on | Use one brightness range for the whole clip. It comes from the min/max values in the file header if present, otherwise from a scan of every frame. Use `--no-cptv-normalize-clip` to stretch each frame on its own instead |
| `--cptv-scale` | `4` | Smooth (bilinear) upscale factor for the 160×120 sensor image, from 1 to 16 |

```bash
vlc --cptv-palette=ironbow --cptv-scale=6 recording.cptv
```

## Metadata

VLC's **Media Information** window shows:

- **Title:** the device name and the recording time (UTC)
- **Date:** the recording time
- **Artist:** the device name
- **Description:** device ID, camera brand/model, serial number, firmware, location, and the sensor value range

## Notes

- Playback runs at the frame rate in the file header, which is 9 fps for current Lepton cameras.
- VLC's built-in `inflate` stream filter stops after the first gzip member. CPTV files are
  multi-member gzip streams, so the demuxer reads the compressed bytes from below that filter
  and inflates them itself.
- The decoder is pinned to a specific `cptv-tools` commit in `Cargo.toml`. To use a newer
  decoder, update `rev`.
