//! Raster image engine: JPG, PNG, WebP, GIF and BMP in, the same out.

use crate::{
    detect_input, Error, ImageFormat, InputKind, Job, Metadata, OutputFormat, Resize, Result,
};
use ::image::{
    codecs::jpeg::JpegEncoder, imageops::FilterType, metadata::Orientation, DynamicImage,
    ImageEncoder, RgbImage, RgbaImage,
};
use img_parts::{jpeg::Jpeg, png::Png, webp::WebP, Bytes, ImageEXIF};
use std::io::Cursor;
use std::sync::atomic::{AtomicBool, Ordering};

fn bad<E: std::error::Error + Send + Sync + 'static>(e: E) -> Error {
    Error::Io(std::io::Error::new(std::io::ErrorKind::InvalidData, e))
}

fn check(cancel: &AtomicBool) -> Result<()> {
    if cancel.load(Ordering::Relaxed) {
        Err(Error::Cancelled)
    } else {
        Ok(())
    }
}

/// Raw TIFF-structured EXIF of the input, when the container can hold one.
fn read_exif(fmt: ImageFormat, data: &[u8]) -> Option<Bytes> {
    let b = Bytes::copy_from_slice(data);
    match fmt {
        ImageFormat::Jpeg => Jpeg::from_bytes(b).ok()?.exif(),
        ImageFormat::Png => Png::from_bytes(b).ok()?.exif(),
        ImageFormat::WebP => WebP::from_bytes(b).ok()?.exif(),
        _ => None,
    }
}

/// Position of the orientation value in IFD0: (offset, value, little endian).
fn orientation_tag(t: &[u8]) -> Option<(usize, u16, bool)> {
    let le = match t.get(..2)? {
        b"II" => true,
        b"MM" => false,
        _ => return None,
    };
    let u16_at = |o: usize| {
        let b: [u8; 2] = t.get(o..o + 2)?.try_into().ok()?;
        Some(if le {
            u16::from_le_bytes(b)
        } else {
            u16::from_be_bytes(b)
        })
    };
    let b: [u8; 4] = t.get(4..8)?.try_into().ok()?;
    let ifd = if le {
        u32::from_le_bytes(b)
    } else {
        u32::from_be_bytes(b)
    } as usize;
    (0..u16_at(ifd)? as usize).find_map(|i| {
        let e = ifd + 2 + i * 12;
        // tag 0x0112, type SHORT
        (u16_at(e)? == 0x0112 && u16_at(e + 2)? == 3).then(|| Some((e + 8, u16_at(e + 8)?, le)))?
    })
}

fn flatten_on_white(img: &DynamicImage) -> RgbImage {
    let rgba = img.to_rgba8();
    let mut out = RgbImage::new(rgba.width(), rgba.height());
    for (o, p) in out.pixels_mut().zip(rgba.pixels()) {
        let a = u32::from(p[3]);
        for c in 0..3 {
            o[c] = ((u32::from(p[c]) * a + 255 * (255 - a) + 127) / 255) as u8;
        }
    }
    out
}

fn resized(img: DynamicImage, resize: Option<Resize>) -> DynamicImage {
    let (w, h) = (img.width(), img.height());
    let scale = match resize {
        None => return img,
        Some(Resize::Percent(p)) => f64::from(p) / 100.0,
        Some(Resize::Fit { max_w, max_h }) => (f64::from(max_w) / f64::from(w))
            .min(f64::from(max_h) / f64::from(h))
            .min(1.0),
        Some(Resize::ShortSide(s)) => (f64::from(s) / f64::from(w.min(h))).min(1.0),
    };
    let dim = |v: u32| ((f64::from(v) * scale).round() as u32).max(1);
    let (nw, nh) = (dim(w), dim(h));
    if (nw, nh) == (w, h) {
        return img;
    }
    img.resize_exact(nw, nh, FilterType::Lanczos3)
}

fn encode(img: &DynamicImage, fmt: ImageFormat, quality: u8) -> Result<Vec<u8>> {
    let q = quality.clamp(1, 100);
    let mut buf = Vec::new();
    match fmt {
        ImageFormat::Jpeg => {
            let rgb = flatten_on_white(img);
            JpegEncoder::new_with_quality(&mut buf, q)
                .write_image(
                    rgb.as_raw(),
                    rgb.width(),
                    rgb.height(),
                    ::image::ExtendedColorType::Rgb8,
                )
                .map_err(bad)?;
        }
        ImageFormat::Bmp => DynamicImage::ImageRgb8(flatten_on_white(img))
            .write_to(&mut Cursor::new(&mut buf), ::image::ImageFormat::Bmp)
            .map_err(bad)?,
        ImageFormat::Png => DynamicImage::ImageRgba8(img.to_rgba8())
            .write_to(&mut Cursor::new(&mut buf), ::image::ImageFormat::Png)
            .map_err(bad)?,
        ImageFormat::Gif => DynamicImage::ImageRgba8(img.to_rgba8())
            .write_to(&mut Cursor::new(&mut buf), ::image::ImageFormat::Gif)
            .map_err(bad)?,
        ImageFormat::WebP => {
            let rgba: RgbaImage = img.to_rgba8();
            let m = webp::Encoder::from_rgba(rgba.as_raw(), rgba.width(), rgba.height())
                .encode(f32::from(q));
            buf = m.to_vec();
        }
        ImageFormat::Heic => return Err(Error::Unsupported("HEIC is input only".into())),
    }
    Ok(buf)
}

/// Copies `exif` (orientation reset to 1) into the encoded image.
fn embed_exif(fmt: ImageFormat, encoded: Vec<u8>, exif: Bytes) -> Result<Vec<u8>> {
    let mut tiff = exif.to_vec();
    if let Some((off, _, le)) = orientation_tag(&tiff) {
        tiff[off..off + 2].copy_from_slice(&if le {
            1u16.to_le_bytes()
        } else {
            1u16.to_be_bytes()
        });
    }
    let exif = Some(Bytes::from(tiff));
    let b = Bytes::from(encoded);
    let mut out = Vec::new();
    match fmt {
        ImageFormat::Jpeg => {
            let mut i = Jpeg::from_bytes(b).map_err(bad)?;
            i.set_exif(exif);
            i.encoder().write_to(&mut out)?;
        }
        ImageFormat::Png => {
            let mut i = Png::from_bytes(b).map_err(bad)?;
            i.set_exif(exif);
            i.encoder().write_to(&mut out)?;
        }
        ImageFormat::WebP => {
            let mut i = WebP::from_bytes(b).map_err(bad)?;
            i.set_exif(exif);
            i.encoder().write_to(&mut out)?;
        }
        _ => unreachable!("caller only embeds for JPEG, PNG and WebP"),
    }
    Ok(out)
}

/// Converts an image job. The output appears only once fully written.
pub fn convert(job: &Job, progress: &dyn Fn(f32), cancel: &AtomicBool) -> Result<()> {
    let OutputFormat::Image(out_fmt) = job.format else {
        return Err(Error::Unsupported("not an image format".into()));
    };
    progress(0.0);
    check(cancel)?;

    let (mut img, exif, rotate) = match detect_input(&job.input)? {
        InputKind::Image(ImageFormat::Heic) => {
            let d = crate::heic::decode(&job.input)?;
            // libheif already applied irot/imir, so the EXIF orientation must not rotate again.
            (
                DynamicImage::ImageRgba8(d.image),
                d.exif.map(Bytes::from),
                false,
            )
        }
        InputKind::Image(f) => {
            let data = std::fs::read(&job.input)?;
            (
                ::image::load_from_memory(&data).map_err(bad)?,
                read_exif(f, &data),
                true,
            )
        }
        InputKind::Video => return Err(Error::Unsupported("video input".into())),
    };
    progress(0.3);
    check(cancel)?;

    if let Some(o) = exif
        .as_deref()
        .filter(|_| rotate)
        .and_then(orientation_tag)
        .and_then(|t| Orientation::from_exif(t.1 as u8))
    {
        img.apply_orientation(o);
    }
    let opts = &job.options;
    let img = resized(img, opts.resize);
    progress(0.6);
    check(cancel)?;

    let mut bytes = encode(&img, out_fmt, opts.quality)?;
    let can_hold = matches!(
        out_fmt,
        ImageFormat::Jpeg | ImageFormat::Png | ImageFormat::WebP
    );
    if let (Metadata::Keep, true, Some(exif)) = (opts.metadata, can_hold, exif) {
        bytes = embed_exif(out_fmt, bytes, exif)?;
    }
    progress(0.9);
    check(cancel)?;

    let name = job.output.file_name().unwrap_or_default().to_string_lossy();
    let tmp = job
        .output
        .with_file_name(format!(".{name}.{}.part", std::process::id()));
    let done = std::fs::write(&tmp, &bytes).and_then(|()| std::fs::rename(&tmp, &job.output));
    if let Err(e) = done {
        let _ = std::fs::remove_file(&tmp);
        return Err(e.into());
    }
    progress(1.0);
    Ok(())
}
