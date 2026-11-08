# Licences

*General principles, not legal advice.*

## Project licence

Morph-Format is dual-licensed under **MIT OR Apache-2.0**, at your option
([LICENSE-MIT](../LICENSE-MIT), [LICENSE-APACHE](../LICENSE-APACHE)).

**No warranty:** the software is provided "as is", without warranty of any kind. The authors are not
liable for any damage, including a file damaged or lost during a conversion (see the warranty
sections of both licences). Always keep your original files.

## Rust crates

The licence texts of every crate used are in [THIRD-PARTY-NOTICES.md](../THIRD-PARTY-NOTICES.md).
They must accompany every distributed binary. `deny.toml` allows only permissive licences compatible
with MIT OR Apache-2.0 (GPL and AGPL are rejected). There are three permissive exceptions, each
justified in `deny.toml`:
- WTFPL, for the `ffmpeg-next` bindings;
- OFL-1.1 and Ubuntu-font-1.0, for the fonts bundled with egui.

## FFmpeg (LGPL): `video` feature

FFmpeg is **not** compiled into the executable: it is linked dynamically. The LGPL obligations are met
as follows:

- **LGPL build only:** no `--enable-gpl`, no `--enable-nonfree`, no x264 or x265. `vcpkg.json` enables
  only vpx, aom, dav1d and opus.
- **Replaceable library:** FFmpeg ships as `.dll` (Windows), `.so` (Linux) or `.dylib` (macOS) files next
  to the application. Users can replace them with their own compatible build.
- **Licence text:** the LGPL v2.1+ ([text](https://www.gnu.org/licenses/old-licenses/lgpl-2.1.html))
  ships with the binaries, together with a notice that the application uses FFmpeg.
- **Source code:** a link to the exact version shipped (https://ffmpeg.org/releases/ or the matching tag
  on https://git.ffmpeg.org/ffmpeg.git) and its configure options (`ffmpeg -buildconf`). Both are in
  `scripts/licences/SOURCES.md`, which is copied into the installer; update it with each release.
- No FFmpeg logo or name in the application's name or icon.

## libheif (LGPL): `heic` feature

libheif (LGPL v3, https://github.com/strukturag/libheif) is linked dynamically, together with libde265
(LGPL v3) for HEVC decoding. The same rules apply: a replaceable dynamic library, the licence text
shipped with it, and a link to the source of the version shipped.

## HEIC test images

`crates/morph-core/tests/fixtures/heic/` holds test images derived from the test data of the
`libheif-rs` crate, under **CC BY-SA 4.0**. The licence text is `LICENSE-CC-BY-SA-4.0` in that folder,
and its `README.md` details where the images come from. They are used **only for tests** and are
**not** shipped in the binary. They keep their own licence (derivatives stay under CC BY-SA 4.0), so
MIT OR Apache-2.0 does not cover them.

## Patents

H.264 and HEVC/H.265 are patented, and a free licence gives no protection against patents.
Morph-Format bundles **neither x264 nor x265**: H.264 encoding goes only through the operating system's
codecs:
- Media Foundation on Windows;
- VideoToolbox on macOS;
- VAAPI or V4L2 on Linux.

Free formats (AV1, VP9, WebM, WebP, PNG, JPEG) are the default. HEIC decoding (libde265) carries the same
risk: have a lawyer review it if the project grows.

## Regenerating the notices

```sh
cargo install cargo-about cargo-deny --locked --features cli   # --features cli is for cargo-about
cargo deny check                                               # licences, advisories, bans, sources
cargo about generate --all-features about.hbs -o THIRD-PARTY-NOTICES.md
```

Regenerate after every dependency change and before every release.
