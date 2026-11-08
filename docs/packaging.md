# Packaging Morph-Format for Windows

Produces an **NSIS installer** (`.exe`) in `dist/`, with HEIC and video enabled.

## Prerequisites

- Windows x64.
- Rust.
- Visual Studio Build Tools. They provide `dumpbin`, which the script uses to check the DLLs.
- LLVM (`winget install LLVM.LLVM`).
- cargo-packager: `cargo install cargo-packager --locked`. It is MIT/Apache, and downloads NSIS on first run, so it needs network access once.
- A `morph-deps` folder. By default it is `%USERPROFILE%\Desktop\dev\morph-deps`; you can override it with the `MORPH_DEPS` variable or with the `-FfmpegDir`, `-HeifDir` and `-VcpkgRoot` parameters. It must contain:
  - `ffmpeg\`: the vcpkg `x64-windows` tree of the LGPL FFmpeg build ([build-ffmpeg.md](build-ffmpeg.md));
  - `heif\`: the vcpkg `x64-windows` tree of libheif + libde265 ([build-libheif.md](build-libheif.md));
  - `vcpkg-root\`: a minimal vcpkg root read by `libheif-sys`. It holds:
    - an empty `.vcpkg-root` file;
    - `installed\vcpkg`, a copy of the folder of the same name from a vcpkg install;
    - `installed\x64-windows`, a junction to `heif\`.

## One command

```powershell
powershell -NoProfile -File scripts\package-windows.ps1
```

The script:
1. runs `cargo build --release -p morph-app --features heic,video`;
2. gathers in `dist\app` the executable, only the release DLLs it needs (checked with `dumpbin /dependents`), and the `licences\` folder. That folder holds MIT, Apache-2.0, the crate notices, LGPL 2.1 and 3, and the sources and configure options of FFmpeg 9.0.2, libheif and libde265;
3. creates `dist\Morph-Format-<version>-windows-x64-setup.exe`.

The installer installs per user, with no administrator rights, to `%LOCALAPPDATA%\Morph-Format`. It adds a Start menu shortcut and an uninstaller.

`-SkipBuild` reuses `target\release\morph-format.exe`. The configuration is in `Packager.toml`. Its version must match `Cargo.toml`, and the script checks this.

## SmartScreen

The installer is **not signed**, so Windows SmartScreen shows "Windows protected your PC". Click **More info**, then **Run anyway**.

## Licences (LGPL)

The FFmpeg, libheif and libde265 DLLs ship as separate files next to the executable and can be replaced
(see [licences.md](licences.md)). The installer includes the `licences\` folder.
