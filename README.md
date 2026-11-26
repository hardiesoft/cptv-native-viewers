# cptv-native-viewers

Native desktop support for [Cacophony Project](https://cacophony.org.nz) **CPTV** thermal
video files on macOS and Linux:

- **VLC plugin** (macOS and Linux): double-click a `.cptv` file to watch it in VLC 3.0, with
  seeking, a choice of colour palettes, and the recording's metadata.
- **Finder thumbnails and spacebar previews** (macOS 13+): Quick Look extensions show a
  thumbnail for each `.cptv` file and play a preview when you press Space.
- **File-manager thumbnails** (Linux): a freedesktop thumbnailer for GNOME Files, Nemo, Caja
  and Thunar.

## Install

Download the installer for your system from the
[latest release](https://github.com/hardiesoft/cptv-native-viewers/releases/latest). You need
[VLC](https://www.videolan.org/vlc/) 3.0 installed too.

| System | File | Installs |
|---|---|---|
| macOS 10.15+ (Apple silicon and Intel) | `cptv-native-viewers-<version>-macos.pkg` | VLC plugin; Finder thumbnails and previews on macOS 13+ |
| Ubuntu 24.04+, Debian 13+ (x86_64) | `cptv-native-viewers_<version>_amd64.deb` | VLC plugin, file-manager thumbnails |
| Fedora (x86_64) | `cptv-native-viewers-<version>-1.x86_64.rpm` | VLC plugin, file-manager thumbnails |

**macOS:** double-click the `.pkg` and follow the installer. If macOS says it can't check
the package for malicious software (releases aren't signed with an Apple Developer ID yet),
open **System Settings → Privacy & Security**, click **Open Anyway** next to the message, and
run it again. Quit and reopen VLC if it was running. The installer has two parts, which you
can choose between under **Customize**:

- **VLC plugin** goes in `/Library/Application Support/cptv-native-viewers/`. A login item puts it on
  VLC's plugin path for every user, and `.cptv` files are set to open in VLC. VLC.app isn't
  changed, so VLC can update itself as normal.
- **Finder thumbnails and previews** installs `/Applications/CPTV Quick Look.app` (macOS 13+).

To remove it, run:

```bash
sudo "/Library/Application Support/cptv-native-viewers/uninstall.sh"
```

**Ubuntu and Debian:** double-click the `.deb` to open it in App Center (or Software), then
click **Install**, or run `sudo apt install ./cptv-native-viewers_*.deb`. **Fedora:**
double-click the `.rpm`, or run `sudo dnf install ./cptv-native-viewers-*.rpm`. Both put the
plugin in the system VLC's own plugin folder, so any VLC can play `.cptv` files. They also add
the thumbnailer (in `/usr/bin`, where GNOME's sandbox allows it), the `video/x-cptv` file type,
and a "VLC (CPTV thermal video)" entry for opening the files. Remove it with
`sudo apt remove cptv-native-viewers` or `sudo dnf remove cptv-native-viewers`. The packages
work with your distribution's VLC, not the Snap or Flatpak versions, which can't load
outside plugins.

If double-clicking a `.cptv` file opens something else, right-click it, choose **Open With**,
pick **VLC** (on Linux, **VLC (CPTV thermal video)**), and make it the default.

## How it works

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
src/bin/           cptv-thumbnailer: freedesktop thumbnailer for Linux file managers
macos/quicklook/   macOS Quick Look thumbnail + preview extensions (Swift, same Rust core)
packaging/         macOS installer (pkg/), LaunchAgent, Linux MIME/desktop/thumbnailer files
scripts/           packaging (package-macos.sh, package-linux.sh) and test helpers
.github/workflows/ release builds
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

## Build from source

You need Rust (stable), a C compiler, `git`, and VLC 3.0.x. On macOS you also need Xcode for
the Quick Look extensions.

```bash
make            # → build/libcptv_plugin.so (Linux) or build/libcptv_plugin.dylib (macOS)
make test       # Rust tests, a headless VLC run that checks frames come out,
                # and a check that the thumbnailer writes a PNG
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

## Install from source

These install what you've built, for development or if you'd rather not use a release
package. Don't combine them with a release package; the macOS installer replaces a per-user
`make install-user` / `make install-quicklook` install it finds.

### Linux, no root

```bash
make install-user
```

This command:

- puts the plugin in `~/.local/lib/cptv-native-viewers/plugins/`
- registers a `video/x-cptv` MIME type for `*.cptv` in `~/.local/share/mime`
- adds a hidden launcher, "VLC (CPTV thermal video)" (`~/.local/share/applications/vlc-cptv.desktop`),
  which runs VLC with `VLC_PLUGIN_PATH` pointing at the plugin
- makes that launcher the default app for `.cptv` files
- installs the file-manager thumbnailer in `~/.local/lib/cptv-native-viewers/` and
  `~/.local/share/thumbnailers/`, but only on desktops that don't sandbox thumbnailers.
  On GNOME and Cinnamon (any system with `bwrap`), it installs nothing for thumbnails and
  tells you to run `sudo make install-thumbnailer` instead. See [Linux file-manager
  thumbnails](#linux-file-manager-thumbnails).

To undo it, run `make uninstall-user`.

The launcher always starts a new VLC window. It doesn't send the file to a VLC that's already
running, because that copy may not have the plugin loaded.

### Linux, system-wide

```bash
make                  # build as yourself first; install targets never build under sudo
sudo make install     # copies into /usr/lib/<triplet>/vlc/plugins/demux and refreshes VLC's plugin cache,
                      # and installs the thumbnailer into /usr/local (set PREFIX=... to change)
make associate        # .cptv → MIME type, and vlc.desktop as the default app
```

This works for any VLC launch, including the command line and "Open With". It won't work
with Snap or Flatpak builds of VLC, which can't see host plugins. Use the distro package.

### Linux file-manager thumbnails

`cptv-thumbnailer` (in `src/bin/`) is a
[freedesktop thumbnailer](https://specifications.freedesktop.org/thumbnail-spec/latest/). It's
registered for `video/x-cptv` by a `cptv.thumbnailer` file and run as
`cptv-thumbnailer -s SIZE INPUT OUTPUT.png`. GNOME Files (Nautilus), Nemo and Caja use it, and
so does Thunar through tumbler. KDE's Dolphin doesn't support this format; it would need a
separate KIO thumbnail plugin.

It draws the same frame as the macOS thumbnails, about a second in (or the last frame of a
shorter clip), with that frame's contrast stretched. It opens clips with the core's
stream-only mode, so it never reads through or holds the whole file, even for older files
without `total_frames`. That takes about 10–60 ms and under 5 MB of memory per file. Before
this, a 10-minute old-format clip took 0.4 s and 207 MB. To
build it on its own, run `make thumbnailer`, which runs
`cargo build --release --features thumbnailer`.

#### Installing on GNOME (and Cinnamon)

GNOME runs thumbnailers in a bubblewrap sandbox that mounts only `/usr`, so a thumbnailer in
your home folder can't run there. It fails with "No such file or directory". It also has to
be the only `cptv.thumbnailer` entry, because an entry in `~/.local/share/thumbnailers`
overrides one in `/usr/local/share/thumbnailers`. To install it:

```bash
make                            # as yourself
sudo make install-thumbnailer   # just the thumbnailer: /usr/local/bin + /usr/local/share/thumbnailers
nautilus -q                     # restart Files
```

`sudo make install` does the same as part of the system-wide install, and
`sudo make uninstall-thumbnailer` removes it. Either way, you also need the `video/x-cptv` MIME
type from `make install-user` or `make associate`.

On desktops that don't sandbox thumbnailers (for example, Thunar through tumbler), `make
install-user` installs a per-user copy that works without root.

GNOME remembers failed thumbnails and won't retry those files. After changing anything, clear
`~/.cache/thumbnails/fail/` and restart the file manager (`nautilus -q`).

#### What's been tested on Linux

On Ubuntu with GNOME Files 50, after `sudo make install-thumbnailer` and a Files restart:

- **Files:** Files generates and caches thumbnails for a folder of `.cptv` clips.
- **Thumbnail factory:** through GNOME's own thumbnail factory (libgnome-desktop-4, which
  Files uses), with its real bubblewrap sandbox and seccomp filter, all upstream samples
  thumbnail at every size Files asks for (128–1024 px).
- **Real recordings:** 38 distinct real recordings and test files were tried. Every valid one
  produced a thumbnail. The deliberately empty and invalid files failed cleanly, and Files
  shows a generic icon for those.

Not yet tried: the per-user install on a file manager that doesn't sandbox thumbnailers (for
example, Thunar).

### macOS, no admin rights

```bash
make install-user
```

This command:

- puts the plugin in `~/Library/Application Support/cptv-native-viewers/plugins/`
- adds a LaunchAgent (`~/Library/LaunchAgents/nz.org.cacophony.cptv-native-viewers.plist`) that runs
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

- `make` builds for this Mac's architecture. For an Intel VLC on Apple silicon, use
  `make ARCH=x86_64 RUST_TARGET=x86_64-apple-darwin BUILD=build/x86_64`, or build the
  universal installer with `make package-macos`.
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
  frame instead of normalising over the whole clip, and opens the clip in stream-only mode
  (`CPTV_OPEN_STREAM_ONLY`), so it never reads through or holds the whole file, even for older
  files without `total_frames`. A clip shorter than a second gives its last frame. Each
  thumbnail takes about 50–80 ms. Thumbnails are drawn like photo
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
hoc, which is enough on your own Mac. The release installer can sign it with a Developer ID
(see [Releases](#releases)).

If thumbnails don't appear, run `qlmanage -r cache` and reopen the Finder window. If a file
type was previously registered differently, Quick Look's thumbnail agent can keep using the
old type until you log out and back in.

## Releases

`.github/workflows/release.yml` builds and tests all three installers:

- **macOS** (`macos-15` runner): `scripts/package-macos.sh` builds the plugin, the Quick Look
  app and a small helper for both arm64 and x86_64, merges them with `lipo`, and wraps them in
  a `.pkg` with `pkgbuild`/`productbuild`. The plugin needs macOS 10.15 (11 on Apple silicon),
  and Quick Look needs 13.
- **Debian/Ubuntu** (`ubuntu-24.04`, x86_64): built against Ubuntu's VLC, so it uses the
  `3_0_0ft64` ABI, and packaged with [nfpm](https://nfpm.goreleaser.com). The job then
  installs the `.deb` and checks that VLC plays a sample with it.
- **Fedora** (`fedora:42` container, x86_64): the same with Fedora's VLC (`3_0_0f`) as an
  `.rpm`.

To publish a release, set `version` in `Cargo.toml`, commit, and push a matching tag:

```bash
git tag v0.1.0
```

```bash
git push origin v0.1.0
```

The workflow then creates a GitHub Release with the three files. You can also create the
release on GitHub first (**Releases → Draft a new release**, with a new `v…` tag): the
workflow then attaches the files to it and keeps your title and notes. If you re-run the
workflow for a tag, it replaces the release's files. Running it by hand
(**Actions → Release → Run workflow**) builds the same files with a `-dev.<commit>` version and
attaches them to the run as artifacts, without releasing.

You can build the packages locally too: `make package-macos` on a Mac, or
`make package-linux FORMAT=deb` (or `rpm`) on the target distro with nfpm installed
(`scripts/install-nfpm.sh 2.48.0`). The output goes in `dist/`.

### Signing the macOS installer

Without signing, the `.pkg` and everything in it are signed ad hoc, so macOS asks users to
approve the installer under **Privacy & Security** first. To sign and notarize it, which
needs a paid Apple Developer account, add these repository secrets:

| Secret | Value |
|---|---|
| `MACOS_CERTIFICATES_P12` | base64 of a `.p12` export holding both your **Developer ID Application** and **Developer ID Installer** certificates and keys |
| `MACOS_CERTIFICATES_PASSWORD` | the `.p12`'s password |
| `APPLE_ID` | your Apple ID email |
| `APPLE_TEAM_ID` | your 10-character team ID |
| `APPLE_APP_PASSWORD` | an [app-specific password](https://support.apple.com/102654) for that Apple ID |

With those set, the code is signed with the hardened runtime, the `.pkg` is signed, and it's
notarized and stapled. The signing path hasn't been run yet, because it needs real
certificates.

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
