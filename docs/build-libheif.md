# Building with HEIC support (libheif)

HEIC/HEIF is **input only** and sits behind the `heic` cargo feature of `morph-core`:

```
cargo test  -p morph-core --features heic
cargo build -p morph-core --features heic
```

libheif is linked **dynamically** (LGPL). HEVC decoding must come from **libde265** (LGPL).
Never ship a libheif built with x265 (GPL): Morph-Format only decodes HEIC, so no encoder is needed.

`libheif-rs` is declared with `default-features = false` and the libheif API version `v1_17`
(in `crates/morph-core/Cargo.toml`).

## Windows (MSVC + vcpkg)

The vcpkg that ships with the VS Build Tools has no ports tree, so clone a full one (once, anywhere
outside the repo):

```powershell
git clone --depth 1 https://github.com/microsoft/vcpkg.git C:\dev\vcpkg
C:\dev\vcpkg\bootstrap-vcpkg.bat -disableMetrics
# NOT plain "libheif": its default features pull x265 (GPL). Ask for core + libde265 only.
C:\dev\vcpkg\vcpkg.exe install "libheif[core,libde265]:x64-windows"

$env:VCPKG_ROOT = "C:\dev\vcpkg"
$env:VCPKGRS_DYNAMIC = "1"                                   # link heif.dll, not a static lib
$env:PATH += ";C:\dev\vcpkg\installed\x64-windows\bin"       # heif.dll + libde265.dll at run time
cargo test -p morph-core --features heic
```

The repo's `vcpkg.json` (manifest mode, `vcpkg install` at the repo root) declares the same
`libheif` with `default-features: false` + `libde265`, but also builds FFmpeg; the classic command
above is quicker when you only need HEIC. To ship the app, copy `heif.dll` and `libde265.dll` next
to the executable together with the LGPL notices.

## macOS

```sh
brew install libheif pkg-config
cargo test -p morph-core --features heic
```

Homebrew's libheif also loads x265 (GPL) as a plugin when present. That is fine for local
development; for a release build libheif from source with `-DWITH_X265=OFF -DWITH_LIBDE265=ON`.

## Linux

```sh
sudo apt install libheif-dev libde265-dev pkg-config     # Debian / Ubuntu
sudo dnf install libheif-devel libde265-devel pkgconf    # Fedora
cargo test -p morph-core --features heic
```

Check that the distribution's libheif has a HEVC decoder (`libheif-plugin-libde265` on newer
Debian/Ubuntu). For a release, build libheif from source with `-DWITH_X265=OFF -DWITH_LIBDE265=ON`.

## What is decoded

- the primary image only (no sequences or live photos), `irot`/`imir`/`clap` applied by libheif;
- alpha is kept (opaque when the file has none); bit depths above 8 (10-bit iPhone HDR, 12-bit)
  are converted to 8-bit RGBA by libheif;
- the EXIF block feeds the usual keep/strip option. Orientation is not applied a second time,
  and is reset to 1 when EXIF is kept.
