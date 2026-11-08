# Sources and build options of the bundled libraries (DLLs)

The DLLs shipped next to `morph-format.exe` are **dynamic, replaceable** libraries: you can replace them
with your own compatible build (same DLL major version, e.g. `avcodec-63.dll`).

Licence texts:
- `LGPL-2.1.txt` for FFmpeg;
- `LGPL-3.0.txt` for libheif and libde265;
- the `*-copyright.txt` files, with the copyright and licence of each library, taken from vcpkg.

Morph-Format uses FFmpeg; it is not endorsed or distributed by the FFmpeg project.

## FFmpeg 9.0.2 (LGPL v2.1 or later)

- Exact source code: tag **n9.0.2**, at https://git.ffmpeg.org/gitweb/ffmpeg.git/tree/refs/tags/n9.0.2
  or https://ffmpeg.org/releases/ffmpeg-9.0.2.tar.xz
- Built with the vcpkg `ffmpeg` port 9.0.2#4 (triplet `x64-windows`, https://github.com/microsoft/vcpkg/tree/master/ports/ffmpeg)
  with the features `avcodec avformat avfilter swscale swresample vpx aom dav1d opus`.
- Main `./configure` options (`avcodec_configuration()`):
  `--toolchain=msvc --enable-shared --disable-static --enable-pic --disable-doc --enable-runtime-cpudetect
  --disable-autodetect --target-os=win32 --enable-w32threads --enable-d3d11va --enable-d3d12va --enable-dxva2
  --enable-mediafoundation --disable-ffmpeg --disable-ffplay --disable-ffprobe --enable-avcodec --disable-avdevice
  --enable-avformat --enable-avfilter --enable-swresample --enable-swscale --enable-libaom --enable-libdav1d
  --enable-libopus --enable-libvpx --enable-schannel --disable-libx264 --disable-libx265 --disable-libopenh264`
  (no `--enable-gpl`, no `--enable-nonfree`).
- Linked libraries:
  - libvpx (BSD);
  - libaom (BSD-2-Clause);
  - dav1d (BSD-2-Clause);
  - Opus (BSD-3-Clause).

## libheif 1.23.7 (LGPL v3 or later)

- Source code: https://github.com/strukturag/libheif/releases/tag/v1.23.7
- vcpkg `libheif` port, default features disabled, `libde265` only: no x265 encoder.

## libde265 1.1.3 (LGPL v3 or later)

- Source code: https://github.com/strukturag/libde265/releases/tag/v1.1.3
