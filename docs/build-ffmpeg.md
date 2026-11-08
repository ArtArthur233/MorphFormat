# Building with FFmpeg (feature `video`)

`morph-core` links FFmpeg **dynamically** through `ffmpeg-next`, and only an **LGPL** build is allowed:
no `x264`/`x265`, no `--enable-gpl`, no `--enable-nonfree`.
The engine needs these FFmpeg parts: `avcodec`, `avformat`, `avfilter`, `swscale`, `swresample`,
and the encoders `libvpx-vp9`, `libaom-av1`, `libopus` (decoders: native H.264/HEVC/VP9/AAC/Opus, `libdav1d`).
MP4 output uses FFmpeg's native `aac` and `gif` encoders plus the operating system's **H.264 encoder**
(never `libx264`/`libopenh264`): `h264_mf` (Windows), `h264_videotoolbox` (macOS), `h264_vaapi` then `h264_v4l2m2m` (Linux).
The engine tries them in that order and opens one as a probe; `morph_core::video::h264_available()` tells
the UI whether MP4 can be offered. GIF needs the `palettegen`/`paletteuse` filters (default in `avfilter`).

Common to every system: `ffmpeg-sys-next` runs bindgen, so **libclang** must be installed, and
`pkg-config` (Linux/macOS) or `VCPKG_ROOT` (Windows) must lead to the FFmpeg development files.
Never enable the `build*` cargo features of `ffmpeg-next`: they compile a static FFmpeg.

## Windows (vcpkg, tested)

The vcpkg bundled with Visual Studio Build Tools has no port tree, so use a full clone.

```powershell
winget install --id LLVM.LLVM -e                     # libclang, installs to C:\Program Files\LLVM
git clone --depth 1 https://github.com/microsoft/vcpkg C:\vcpkg
C:\vcpkg\bootstrap-vcpkg.bat -disableMetrics

# from the repository root: installs what vcpkg.json declares (LGPL FFmpeg + libheif)
$env:VCPKG_ROOT = "C:\vcpkg"
C:\vcpkg\vcpkg.exe install --triplet x64-windows

# build (dynamic triplet: DLLs, not static)
$env:LIBCLANG_PATH = "C:\Program Files\LLVM\bin"
$env:FFMPEG_DIR = "$PWD\vcpkg_installed\x64-windows"
$env:PATH = "$env:FFMPEG_DIR\bin;$env:PATH"          # the FFmpeg DLLs, also needed to run tests
cargo build -p morph-core --features video
cargo test  -p morph-core --features video
```

`vcpkg_installed/` is created in the repository root; it is build output, do not commit it
(`--x-install-root=<dir>` puts it elsewhere; then point `FFMPEG_DIR` at `<dir>\x64-windows`).

## Linux (Debian/Ubuntu)

```sh
sudo apt install clang libclang-dev pkg-config \
  libavcodec-dev libavformat-dev libavfilter-dev libavutil-dev libswscale-dev libswresample-dev \
  libvpx-dev libaom-dev libopus-dev
cargo build -p morph-core --features video
```

Debian/Ubuntu's FFmpeg is configured `--enable-gpl` (because of x264/x265), which this project must
not ship. For development that is fine; for releases build your own LGPL FFmpeg:

```sh
./configure --prefix=$HOME/ffmpeg-lgpl --enable-shared --disable-static --disable-programs \
  --enable-libvpx --enable-libaom --enable-libopus --enable-libdav1d
make -j"$(nproc)" && make install
export PKG_CONFIG_PATH=$HOME/ffmpeg-lgpl/lib/pkgconfig LD_LIBRARY_PATH=$HOME/ffmpeg-lgpl/lib
```

(omit `--disable-programs` if you want an `ffmpeg` binary to run the checks below).

## macOS (Homebrew)

```sh
brew install pkg-config llvm libvpx aom opus dav1d
brew install ffmpeg          # Homebrew's is GPL (x264): for releases build as on Linux
export LIBCLANG_PATH="$(brew --prefix llvm)/lib"
cargo build -p morph-core --features video
```

## System H.264 encoders per OS

| OS | FFmpeg configure flag | Encoder |
|----|----------------------|---------|
| Windows | `--enable-mediafoundation` (on by default with MSVC/mingw; the vcpkg `ffmpeg` port has it) | `h264_mf` |
| macOS | `--enable-videotoolbox` (default when the SDK is present) | `h264_videotoolbox` |
| Linux | `--enable-vaapi` (needs `libva-dev`; Intel/AMD, device `/dev/dri/renderD128`, frames uploaded with `hwupload`); `--enable-v4l2-m2m` (default, mostly ARM) | `h264_vaapi`, `h264_v4l2m2m` |

These are LGPL-compatible: they call the OS codec and bundle none. Check with
`ffmpeg -encoders | grep -E "h264_mf|h264_videotoolbox|h264_vaapi|h264_v4l2m2m"`. If none is listed or none opens,
MP4 conversion fails with "no system H.264 encoder available" and `h264_available()` is false.
Checked on the Windows vcpkg build above: `h264_mf` is present and produces H.264 + AAC MP4. `h264_vaapi` is implemented but untested (no Linux GPU available when written).

## Verify the build is LGPL

With an `ffmpeg` binary from the same build:

```sh
ffmpeg -L              # must print the "GNU Lesser General Public License" text
ffmpeg -version        # the "configuration:" line must NOT contain
                       #   --enable-gpl  --enable-nonfree  --enable-libx264  --enable-libx265
ffmpeg -encoders | grep -E "libvpx-vp9|libaom-av1|libopus"   # all three must be listed
```

Without the binary, the library tells the same thing: `avcodec_license()` returns
`"LGPL version 2.1 or later"` for an LGPL build (`"GPL version 2 or later"` otherwise). On Windows the
DLL itself carries it: `Select-String -Path vcpkg_installed\x64-windows\bin\avcodec-*.dll -Pattern "LGPL version","--enable-gpl" -Encoding ascii`
must match only the first. (Checked on the vcpkg build above: LGPL 2.1, configured with libaom, dav1d, opus, vpx only.)

## Checks

`cargo build -p morph-core --features video` then `cargo test -p morph-core --features video`.
Without the feature nothing needs FFmpeg: `cargo test --workspace` builds and runs without it.
