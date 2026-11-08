//! HEIC decoding (libheif, feature `heic`).

use crate::{Error, Result};
use std::path::Path;

/// A decoded HEIC primary image.
pub struct Decoded {
    /// 8-bit RGBA pixels, with irot/imir/clap already applied by libheif.
    pub image: ::image::RgbaImage,
    /// Raw TIFF-structured EXIF block, if the file has one.
    pub exif: Option<Vec<u8>>,
}

/// Decodes the primary image of a HEIC file; 10-bit sources are converted to 8-bit.
#[cfg(feature = "heic")]
pub fn decode(path: &Path) -> Result<Decoded> {
    use libheif_rs::{ColorSpace, HeifContext, LibHeif, RgbChroma};

    let bad = |e: libheif_rs::HeifError| {
        Error::Io(std::io::Error::new(std::io::ErrorKind::InvalidData, e))
    };
    let ctx = HeifContext::read_from_file(&path.to_string_lossy()).map_err(bad)?;
    let handle = ctx.primary_image_handle().map_err(bad)?;
    // RGBA interleaved 8-bit makes libheif convert any bit depth and fill opaque alpha.
    let img = LibHeif::new()
        .decode(&handle, ColorSpace::Rgb(RgbChroma::Rgba), None)
        .map_err(bad)?;
    let plane = img
        .planes()
        .interleaved
        .ok_or_else(|| Error::Unsupported("HEIC image without interleaved pixels".into()))?;

    let (w, h) = (plane.width as usize, plane.height as usize);
    let mut raw = Vec::with_capacity(w * h * 4);
    for row in plane.data.chunks(plane.stride).take(h) {
        raw.extend_from_slice(&row[..w * 4]);
    }
    let image = ::image::RgbaImage::from_raw(plane.width, plane.height, raw)
        .ok_or_else(|| Error::Unsupported("HEIC pixel buffer size mismatch".into()))?;

    let mut ids = [0; 4];
    let n = handle.metadata_block_ids(&mut ids, b"Exif");
    let exif = ids[..n.min(4)].iter().find_map(|&id| {
        let b = handle.metadata(id).ok()?;
        // The block starts with the offset (u32 BE) from itself to the TIFF header.
        let off = u32::from_be_bytes(b.get(..4)?.try_into().ok()?) as usize;
        b.get(4 + off..).map(<[u8]>::to_vec)
    });
    Ok(Decoded { image, exif })
}

/// Decodes a HEIC file; this build has no `heic` feature.
#[cfg(not(feature = "heic"))]
pub fn decode(_path: &Path) -> Result<Decoded> {
    Err(Error::FeatureDisabled("HEIC"))
}
