# HEIC test samples

All files derive from the test data of the [`libheif-rs`](https://github.com/Cykooz/libheif-rs)
crate (`data/`), licensed **CC BY-SA 4.0** (full text: `LICENSE-CC-BY-SA-4.0`). Derivatives are
shared under the same licence.

| File | Origin |
| --- | --- |
| `alpha.heif` | `data/alpha.heif` unchanged: 256x256, HEVC 8-bit, separate alpha plane. |
| `12bit.heif` | `data/test_nclx.heif` unchanged: 2048x1440, HEVC 12-bit (used for the 8-bit downconversion test). |
| `oriented-exif.heic` | `alpha.heif` with boxes added by hand: an `irot` (90 degrees counter-clockwise) and an `imir` transform on the primary image, and an `Exif` item (Make "Morph", Orientation 6, GPS latitude 48 deg 51 min N, a synthetic value). |
