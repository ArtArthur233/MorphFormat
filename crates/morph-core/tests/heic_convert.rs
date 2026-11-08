//! HEIC input through libheif into every raster output.
#![cfg(feature = "heic")]

use image::{imageops, GenericImageView, RgbaImage};
use img_parts::{jpeg::Jpeg, png::Png, webp::WebP, ImageEXIF};
use morph_core::{
    convert, heic, ConvertOptions, ImageFormat as F, Job, Metadata, OutputFormat, Resize,
};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

const ALL: [F; 5] = [F::Jpeg, F::Png, F::WebP, F::Gif, F::Bmp];

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/heic")
        .join(name)
}

fn run(name: &str, out: &Path, fmt: F, options: ConvertOptions) -> morph_core::Result<()> {
    let job = Job {
        input: fixture(name),
        output: out.into(),
        format: OutputFormat::Image(fmt),
        options,
    };
    convert(&job, &|_| {}, &AtomicBool::new(false))
}

fn exif_of(fmt: F, data: Vec<u8>) -> Option<Vec<u8>> {
    let b = img_parts::Bytes::from(data);
    match fmt {
        F::Jpeg => Jpeg::from_bytes(b).unwrap().exif(),
        F::Png => Png::from_bytes(b).unwrap().exif(),
        F::WebP => WebP::from_bytes(b).unwrap().exif(),
        _ => unreachable!(),
    }
    .map(|e| e.to_vec())
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

#[test]
fn decodes_primary_image_with_alpha() {
    let d = heic::decode(&fixture("alpha.heif")).unwrap();
    assert_eq!(d.image.dimensions(), (256, 256));
    assert!(d.image.pixels().any(|p| p[3] < 255), "alpha channel lost");
    assert!(d.image.pixels().any(|p| p[3] == 255));
    assert!(d.exif.is_none());
}

#[test]
fn applies_irot_then_imir() {
    let plain = heic::decode(&fixture("alpha.heif")).unwrap().image;
    let d = heic::decode(&fixture("oriented-exif.heic")).unwrap();
    // irot angle 1 = 90 degrees counter-clockwise, then imir (libheif flips the rotated image top-to-bottom for axis 0).
    let rotated: RgbaImage = imageops::rotate270(&plain);
    assert!(rotated != plain, "fixture must not be symmetric");
    let candidates = [
        ("rot, fliph", imageops::flip_horizontal(&rotated)),
        ("rot, flipv", imageops::flip_vertical(&rotated)),
    ];
    let hits: Vec<_> = candidates
        .iter()
        .filter(|c| c.1 == d.image)
        .map(|c| c.0)
        .collect();
    assert_eq!(hits, ["rot, flipv"], "orientation mismatch");
    // The EXIF orientation (6) is in the file but must not rotate a second time.
    assert!(d.exif.is_some());
}

#[test]
fn high_bit_depth_is_downconverted_to_8_bit() {
    let d = heic::decode(&fixture("12bit.heif")).unwrap();
    assert_eq!(d.image.dimensions(), (2048, 1440));
    assert!(d.image.pixels().any(|p| p[0] > 0 && p[0] < 255));
}

#[test]
fn every_format_with_quality_resize_and_alpha() {
    let dir = tempfile::tempdir().unwrap();
    for fmt in ALL {
        let out = dir.path().join(format!("a.{}", fmt.extension()));
        let o = ConvertOptions {
            quality: 40,
            resize: Some(Resize::Fit {
                max_w: 100,
                max_h: 100,
            }),
            ..Default::default()
        };
        run("alpha.heif", &out, fmt, o).unwrap();
        let img = image::open(&out).unwrap();
        assert_eq!(img.dimensions(), (100, 100), "{fmt:?}");
    }
    // PNG and WebP keep transparency.
    for fmt in [F::Png, F::WebP] {
        let out = dir.path().join(format!("full.{}", fmt.extension()));
        run("alpha.heif", &out, fmt, ConvertOptions::default()).unwrap();
        let img = image::open(&out).unwrap().to_rgba8();
        assert!(img.pixels().any(|p| p[3] < 255), "{fmt:?} lost alpha");
    }
}

#[test]
fn quality_changes_jpeg_size() {
    let dir = tempfile::tempdir().unwrap();
    let size = |q| {
        let out = dir.path().join(format!("q{q}.jpg"));
        let o = ConvertOptions {
            quality: q,
            ..Default::default()
        };
        run("12bit.heif", &out, F::Jpeg, o).unwrap();
        std::fs::metadata(out).unwrap().len()
    };
    assert!(size(20) < size(95));
}

#[test]
fn exif_keep_and_strip() {
    let dir = tempfile::tempdir().unwrap();
    for fmt in [F::Jpeg, F::Png, F::WebP] {
        let ext = fmt.extension();
        let kept = dir.path().join(format!("keep.{ext}"));
        let o = ConvertOptions {
            metadata: Metadata::Keep,
            ..Default::default()
        };
        run("oriented-exif.heic", &kept, fmt, o).unwrap();
        let exif = exif_of(fmt, std::fs::read(&kept).unwrap()).expect("EXIF kept");
        assert!(contains(&exif, b"Morph\0"), "{fmt:?} Make");
        assert!(contains(&exif, &[0x25, 0x88]), "{fmt:?} GPS IFD pointer");
        // Pixels are already upright, so orientation is reset to 1.
        assert!(
            contains(&exif, &[0x12, 0x01, 3, 0, 1, 0, 0, 0, 1, 0]),
            "{fmt:?}"
        );

        let stripped = dir.path().join(format!("strip.{ext}"));
        run(
            "oriented-exif.heic",
            &stripped,
            fmt,
            ConvertOptions::default(),
        )
        .unwrap();
        assert!(exif_of(fmt, std::fs::read(&stripped).unwrap()).is_none());
    }
}

#[test]
fn orientation_is_not_applied_twice() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("o.png");
    run(
        "oriented-exif.heic",
        &out,
        F::Png,
        ConvertOptions::default(),
    )
    .unwrap();
    let expected = heic::decode(&fixture("oriented-exif.heic")).unwrap().image;
    assert_eq!(image::open(&out).unwrap().to_rgba8(), expected);
}
