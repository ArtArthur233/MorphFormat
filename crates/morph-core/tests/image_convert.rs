use image::{GenericImageView, Rgba, RgbaImage};
use img_parts::{jpeg::Jpeg, png::Png, webp::WebP, Bytes, ImageEXIF};
use morph_core::{
    convert, ConvertOptions, Error, ImageFormat as F, Job, Metadata, OutputFormat, Resize,
};
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

const ALL: [F; 5] = [F::Jpeg, F::Png, F::WebP, F::Gif, F::Bmp];

/// Little-endian EXIF: orientation + a GPS IFD with one entry.
fn exif(orientation: u16) -> Bytes {
    let mut t = b"II*\0\x08\0\0\0".to_vec();
    t.extend(2u16.to_le_bytes());
    for (tag, ty, val) in [(0x0112u16, 3u16, orientation as u32), (0x8825, 4, 38)] {
        t.extend(tag.to_le_bytes());
        t.extend(ty.to_le_bytes());
        t.extend(1u32.to_le_bytes());
        t.extend(val.to_le_bytes());
    }
    t.extend(0u32.to_le_bytes());
    // GPS IFD at offset 38: one ASCII entry (GPSLatitudeRef = "N")
    t.extend(1u16.to_le_bytes());
    t.extend(1u16.to_le_bytes());
    t.extend(2u16.to_le_bytes());
    t.extend(2u32.to_le_bytes());
    t.extend(b"N\0\0\0");
    t.extend(0u32.to_le_bytes());
    Bytes::from(t)
}

fn gradient(w: u32, h: u32) -> RgbaImage {
    RgbaImage::from_fn(w, h, |x, y| {
        Rgba([(x * 255 / w) as u8, (y * 255 / h) as u8, 128, 255])
    })
}

fn noise(w: u32, h: u32) -> RgbaImage {
    let mut s = 12345u32;
    RgbaImage::from_fn(w, h, |_, _| {
        s = s.wrapping_mul(1664525).wrapping_add(1013904223);
        Rgba([(s >> 8) as u8, (s >> 16) as u8, (s >> 24) as u8, 255])
    })
}

fn run(input: &Path, output: &Path, fmt: F, options: ConvertOptions) -> morph_core::Result<()> {
    let job = Job {
        input: input.into(),
        output: output.into(),
        format: OutputFormat::Image(fmt),
        options,
    };
    convert(&job, &|_| {}, &AtomicBool::new(false))
}

/// Writes `img` as `fmt` into `dir`, optionally with EXIF injected.
fn write_src(dir: &Path, img: &RgbaImage, fmt: F, exif_bytes: Option<Bytes>) -> PathBuf {
    let seed = dir.join("seed.png");
    img.save(&seed).unwrap();
    let out = dir.join(format!("src.{}", fmt.extension()));
    let o = ConvertOptions {
        quality: 95,
        ..Default::default()
    };
    if fmt == F::Png {
        std::fs::copy(&seed, &out).unwrap();
    } else {
        run(&seed, &out, fmt, o).unwrap();
    }
    if let Some(e) = exif_bytes {
        let b = Bytes::from(std::fs::read(&out).unwrap());
        let mut w = Vec::new();
        match fmt {
            F::Jpeg => {
                let mut i = Jpeg::from_bytes(b).unwrap();
                i.set_exif(Some(e));
                i.encoder().write_to(&mut w).unwrap();
            }
            F::Png => {
                let mut i = Png::from_bytes(b).unwrap();
                i.set_exif(Some(e));
                i.encoder().write_to(&mut w).unwrap();
            }
            F::WebP => {
                let mut i = WebP::from_bytes(b).unwrap();
                i.set_exif(Some(e));
                i.encoder().write_to(&mut w).unwrap();
            }
            _ => panic!("no exif"),
        }
        std::fs::write(&out, w).unwrap();
    }
    out
}

fn exif_of(path: &Path, fmt: F) -> Option<Bytes> {
    let b = Bytes::from(std::fs::read(path).unwrap());
    match fmt {
        F::Jpeg => Jpeg::from_bytes(b).unwrap().exif(),
        F::Png => Png::from_bytes(b).unwrap().exif(),
        F::WebP => WebP::from_bytes(b).unwrap().exif(),
        _ => None,
    }
}

#[test]
fn every_format_pair() {
    let dir = tempfile::tempdir().unwrap();
    for from in ALL {
        let src = write_src(dir.path(), &gradient(16, 8), from, None);
        for to in ALL {
            let out = dir
                .path()
                .join(format!("{}_to.{}", from.extension(), to.extension()));
            run(&src, &out, to, ConvertOptions::default()).unwrap();
            let want = match to {
                F::Jpeg => image::ImageFormat::Jpeg,
                F::Png => image::ImageFormat::Png,
                F::WebP => image::ImageFormat::WebP,
                F::Gif => image::ImageFormat::Gif,
                _ => image::ImageFormat::Bmp,
            };
            let data = std::fs::read(&out).unwrap();
            assert_eq!(
                image::guess_format(&data).unwrap(),
                want,
                "{from:?}->{to:?}"
            );
            assert_eq!(
                image::load_from_memory(&data).unwrap().dimensions(),
                (16, 8)
            );
        }
    }
}

#[test]
fn quality_drives_jpeg_and_webp_size() {
    let dir = tempfile::tempdir().unwrap();
    let src = write_src(dir.path(), &noise(64, 64), F::Png, None);
    for f in [F::Jpeg, F::WebP] {
        let size = |q| {
            let out = dir.path().join(format!("q{q}.{}", f.extension()));
            let o = ConvertOptions {
                quality: q,
                ..Default::default()
            };
            run(&src, &out, f, o).unwrap();
            std::fs::metadata(out).unwrap().len()
        };
        assert!(size(10) < size(95), "{f:?}");
    }
}

#[test]
fn resize_keeps_ratio_and_never_upscales_fit() {
    let dir = tempfile::tempdir().unwrap();
    let src = write_src(dir.path(), &gradient(40, 20), F::Png, None);
    let dims = |r, name: &str| {
        let out = dir.path().join(name);
        let o = ConvertOptions {
            resize: Some(r),
            ..Default::default()
        };
        run(&src, &out, F::Png, o).unwrap();
        image::open(out).unwrap().dimensions()
    };
    assert_eq!(dims(Resize::Percent(50), "a.png"), (20, 10));
    assert_eq!(dims(Resize::ShortSide(5), "a.png"), (10, 5));
    assert_eq!(dims(Resize::ShortSide(1000), "a.png"), (40, 20));
    assert_eq!(
        dims(
            Resize::Fit {
                max_w: 10,
                max_h: 10
            },
            "b.png"
        ),
        (10, 5)
    );
    assert_eq!(
        dims(
            Resize::Fit {
                max_w: 400,
                max_h: 400
            },
            "c.png"
        ),
        (40, 20)
    );
}

#[test]
fn alpha_is_flattened_on_white() {
    let dir = tempfile::tempdir().unwrap();
    let clear = RgbaImage::from_pixel(8, 8, Rgba([0, 0, 0, 0]));
    let src = write_src(dir.path(), &clear, F::Png, None);
    for f in [F::Jpeg, F::Bmp] {
        let out = dir.path().join(format!("w.{}", f.extension()));
        run(&src, &out, f, ConvertOptions::default()).unwrap();
        let p = image::open(out).unwrap().to_rgb8().get_pixel(4, 4).0;
        assert!(p.iter().all(|&c| c >= 250), "{f:?} {p:?}");
    }
}

#[test]
fn orientation_is_applied_then_reset_when_kept() {
    let dir = tempfile::tempdir().unwrap();
    for from in [F::Jpeg, F::Png, F::WebP] {
        let src = write_src(dir.path(), &gradient(16, 8), from, Some(exif(6)));
        for to in [F::Jpeg, F::Png, F::WebP, F::Gif] {
            let out = dir
                .path()
                .join(format!("o_{}.{}", from.extension(), to.extension()));
            let o = ConvertOptions {
                metadata: Metadata::Keep,
                ..Default::default()
            };
            run(&src, &out, to, o).unwrap();
            assert_eq!(image::open(&out).unwrap().dimensions(), (8, 16));
            if to != F::Gif {
                let e = exif_of(&out, to).expect("exif kept");
                // IFD0 entry 0 value: header 8 + count 2 + tag/type/count 8
                assert_eq!(e[18], 1, "orientation reset");
                assert!(e.windows(2).any(|w| w == [0x25, 0x88]), "GPS kept");
            }
        }
    }
}

#[test]
fn strip_writes_no_exif_or_gps() {
    let dir = tempfile::tempdir().unwrap();
    let src = write_src(dir.path(), &gradient(16, 8), F::Jpeg, Some(exif(6)));
    assert!(exif_of(&src, F::Jpeg).is_some());
    for to in ALL {
        let out = dir.path().join(format!("s.{}", to.extension()));
        run(&src, &out, to, ConvertOptions::default()).unwrap();
        assert_eq!(image::open(&out).unwrap().dimensions(), (8, 16));
        assert!(exif_of(&out, to).is_none(), "{to:?}");
        let raw = std::fs::read(&out).unwrap();
        assert!(!raw.windows(4).any(|w| w == b"Exif" || w == b"EXIF"));
    }
}

#[test]
fn progress_reported_and_cancel_leaves_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let src = write_src(dir.path(), &gradient(8, 8), F::Png, None);
    let out = dir.path().join("p.jpg");
    let job = Job {
        input: src,
        output: out.clone(),
        format: OutputFormat::Image(F::Jpeg),
        options: ConvertOptions::default(),
    };
    let seen = RefCell::new(vec![]);
    convert(
        &job,
        &|p| seen.borrow_mut().push(p),
        &AtomicBool::new(false),
    )
    .unwrap();
    let seen = seen.into_inner();
    assert_eq!((seen[0], *seen.last().unwrap()), (0.0, 1.0));

    std::fs::remove_file(&out).unwrap();
    let before = std::fs::read_dir(dir.path()).unwrap().count();
    let r = convert(&job, &|_| {}, &AtomicBool::new(true));
    assert!(matches!(r, Err(Error::Cancelled)));
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), before);
}

#[test]
fn bad_input_fails_without_output() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("bad.png");
    std::fs::write(&src, b"\x89PNG\r\n\x1a\nnot really").unwrap();
    let out = dir.path().join("bad.jpg");
    assert!(run(&src, &out, F::Jpeg, ConvertOptions::default()).is_err());
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
}
