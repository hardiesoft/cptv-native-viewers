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
macos/quicklook/   macOS Quick Look thumbnail + preview extensions (Swift, same Rust core)
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

### macOS, no admin rights (recommended)

```bash
make install-user
```

This command:

- puts the plugin in `~/Library/Application Support/cptv-vlc/plugins/`
- adds a LaunchAgent (`~/Library/LaunchAgents/nz.org.cacophony.cptv-vlc.plist`) that runs
  `launchctl setenv VLC_PLUGIN_PATH …` at each login, and runs it now. VLC 3 has no per-user
  plugin folder, but every app started from Finder, the Dock or Spotlight inherits that
  variable, so your normal VLC loads the plugin. VLC still loads its own plugins too.
- makes VLC the default app for `.cptv` files

`.cptv` files then open in your one normal VLC, the same as any other video, and
`/Applications/VLC.app` is untouched, so it keeps working after VLC updates itself. If VLC was
already running when you installed, quit and reopen it once. Run `make install-user` again
after rebuilding the plugin. To undo it, run `make uninstall-user`.

Tested with VLC 3.0.24 on Apple Silicon (macOS 15).

### macOS, inside VLC.app

```bash
make install          # copies the ad-hoc-signed dylib into /Applications/VLC.app/Contents/MacOS/plugins
make associate        # uses `duti` if installed; otherwise:
                      #   Finder → select a .cptv → Get Info → Open with: VLC → Change All…
```

macOS App Management protection blocks writes into app bundles. To allow them, give your
terminal app permission under **System Settings → Privacy & Security → App Management**.
VLC.app is built with library validation disabled, so it loads the ad-hoc-signed plugin
without re-signing. A VLC update replaces the app bundle and removes the plugin.

Notes for macOS:

- Build on the same architecture as your VLC.app (arm64 or x86_64).
- To run once without installing anything:
  `VLC_PLUGIN_PATH=$PWD/build /Applications/VLC.app/Contents/MacOS/VLC file.cptv`.
- To use a VLC.app somewhere else, set `VLC_APP=/path/to/VLC.app`.

## macOS Quick Look: Finder thumbnails and spacebar preview

```bash
make install-quicklook
```

This builds `~/Applications/CPTV Quick Look.app` and registers it. The app is just a container
(opening it only says it's installed). It holds two Quick Look extensions that call the same
Rust core through `vlc/cptv_core.h`:

- **Thumbnail**: Finder icons and gallery view show a frame about one second into the clip.
  CPTV frames are delta-coded in one gzip stream, so frame *N* costs *N* decodes. That's why
  it uses an early frame rather than the middle one. It also stretches the contrast of that one
  frame instead of normalising over the whole clip, so it never reads through the whole file.
  Each thumbnail takes about 50–80 ms. Older files without `total_frames` are the exception:
  they're decoded fully into memory when they open. Thumbnails are drawn like photo
  thumbnails, with rounded corners and a drop shadow, rather than inside a document icon with a
  folded corner. That uses a private `QLThumbnailReply` property (`iconFlavor` = 1, the old
  "shadow" style), which the code sets only if it exists, so if a future macOS removes it,
  thumbnails go back to the folded-corner style.
- **Preview**: pressing Space plays the clip in a loop at its own frame rate, with the
  recording's title and a time and frame counter underneath. Click the picture to pause.
  Hover over the title to see the camera details.

The app also declares the `.cptv` file type (`nz.org.cacophony.cptv`), which Quick Look needs
to match the extensions, so Finder now shows the kind as "CPTV thermal video". VLC stays the
app that opens the files. To undo it, run `make uninstall-quicklook`.

It's built with plain `swiftc` (no Xcode project, but Xcode must be installed) and signed ad
hoc, which is enough on your own Mac. To give it to other people without Gatekeeper warnings,
it would need signing with an Apple Developer ID and notarizing.

If thumbnails don't appear, run `qlmanage -r cache` and reopen the Finder window. If a file
type was previously registered differently, Quick Look's thumbnail agent can keep using the
old type until you log out and back in.

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
