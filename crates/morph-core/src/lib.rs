//! Morph-Format engine: local image and video conversion.

pub mod error;
pub mod heic;
pub mod image;
pub mod paths;
pub mod types;
pub mod video;

pub use error::{Error, Result};
pub use paths::output_path;
pub use types::*;

use std::io::Read;
use std::path::Path;
use std::sync::atomic::AtomicBool;

/// Detects the kind of `path` by magic bytes, falling back to the extension.
pub fn detect_input(path: &Path) -> Result<InputKind> {
    let mut head = [0u8; 32];
    let n = std::fs::File::open(path)?.read(&mut head)?;
    sniff(&head[..n])
        .or_else(|| by_extension(path))
        .ok_or_else(|| Error::UnknownInput(path.to_path_buf()))
}

fn sniff(h: &[u8]) -> Option<InputKind> {
    use ImageFormat::*;
    let img = |f| Some(InputKind::Image(f));
    if h.starts_with(&[0xFF, 0xD8, 0xFF]) {
        img(Jpeg)
    } else if h.starts_with(b"\x89PNG\r\n\x1a\n") {
        img(Png)
    } else if h.starts_with(b"GIF87a") || h.starts_with(b"GIF89a") {
        img(Gif)
    } else if h.starts_with(b"BM") {
        img(Bmp)
    } else if h.len() >= 12 && &h[..4] == b"RIFF" && &h[8..12] == b"WEBP" {
        img(WebP)
    } else if h.len() >= 12 && &h[4..8] == b"ftyp" {
        match &h[8..12] {
            b"heic" | b"heix" | b"hevc" | b"hevx" | b"heim" | b"heis" | b"mif1" | b"msf1" => {
                img(Heic)
            }
            _ => Some(InputKind::Video), // mp4, mov, m4v...
        }
    } else if h.starts_with(&[0x1A, 0x45, 0xDF, 0xA3]) // Matroska / WebM
        || (h.len() >= 12 && &h[..4] == b"RIFF" && &h[8..12] == b"AVI ")
    {
        Some(InputKind::Video)
    } else {
        None
    }
}

fn by_extension(path: &Path) -> Option<InputKind> {
    use ImageFormat::*;
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    Some(match ext.as_str() {
        "jpg" | "jpeg" => InputKind::Image(Jpeg),
        "png" => InputKind::Image(Png),
        "webp" => InputKind::Image(WebP),
        "gif" => InputKind::Image(Gif),
        "bmp" => InputKind::Image(Bmp),
        "heic" | "heif" => InputKind::Image(Heic),
        "mp4" | "m4v" | "mov" | "mkv" | "webm" | "avi" => InputKind::Video,
        _ => return None,
    })
}

/// Output formats offered for an input kind (HEIC is never an output).
pub fn supported_outputs(kind: InputKind) -> Vec<OutputFormat> {
    match kind {
        InputKind::Image(_) => {
            use ImageFormat::*;
            [Jpeg, Png, WebP, Gif, Bmp]
                .into_iter()
                .map(OutputFormat::Image)
                .collect()
        }
        InputKind::Video => {
            use VideoFormat::*;
            [WebmVp9, WebmAv1, MkvRemux, Mp4H264, Gif]
                .into_iter()
                .map(OutputFormat::Video)
                .collect()
        }
    }
}

/// Runs a conversion job, reporting progress in 0.0..=1.0 and honouring `cancel`.
pub fn convert(job: &Job, progress: &dyn Fn(f32), cancel: &AtomicBool) -> Result<()> {
    match job.format {
        OutputFormat::Image(ImageFormat::Heic) => {
            Err(Error::Unsupported("HEIC is input only".into()))
        }
        OutputFormat::Image(_) => image::convert(job, progress, cancel),
        OutputFormat::Video(_) => video::convert(job, progress, cancel),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn detect_bytes(name: &str, bytes: &[u8]) -> Result<InputKind> {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join(name);
        std::fs::write(&p, bytes).unwrap();
        detect_input(&p)
    }

    #[test]
    fn detects_by_magic_ignoring_extension() {
        let png = detect_bytes("x.dat", b"\x89PNG\r\n\x1a\n0000").unwrap();
        assert_eq!(png, InputKind::Image(ImageFormat::Png));
        let jpg = detect_bytes("x.png", &[0xFF, 0xD8, 0xFF, 0xE0, 0]).unwrap();
        assert_eq!(jpg, InputKind::Image(ImageFormat::Jpeg));
        let webp = detect_bytes("x", b"RIFF\0\0\0\0WEBPVP8 ").unwrap();
        assert_eq!(webp, InputKind::Image(ImageFormat::WebP));
        let heic = detect_bytes("x", b"\0\0\0\x18ftypheic\0\0\0\0").unwrap();
        assert_eq!(heic, InputKind::Image(ImageFormat::Heic));
        let mp4 = detect_bytes("x", b"\0\0\0\x18ftypisom\0\0\0\0").unwrap();
        assert_eq!(mp4, InputKind::Video);
        let mkv = detect_bytes("x", &[0x1A, 0x45, 0xDF, 0xA3, 0]).unwrap();
        assert_eq!(mkv, InputKind::Video);
    }

    #[test]
    fn falls_back_to_extension_then_fails() {
        assert_eq!(
            detect_bytes("a.JPG", b"????").unwrap(),
            InputKind::Image(ImageFormat::Jpeg)
        );
        assert!(matches!(
            detect_bytes("a.txt", b"hello"),
            Err(Error::UnknownInput(_))
        ));
    }

    #[test]
    fn supported_outputs_per_kind() {
        let img = supported_outputs(InputKind::Image(ImageFormat::Heic));
        assert!(img.contains(&OutputFormat::Image(ImageFormat::Jpeg)));
        assert!(!img.contains(&OutputFormat::Image(ImageFormat::Heic)));
        assert!(img.iter().all(|f| matches!(f, OutputFormat::Image(_))));
        let vid = supported_outputs(InputKind::Video);
        assert!(vid.contains(&OutputFormat::Video(VideoFormat::WebmVp9)));
        assert!(vid.iter().all(|f| matches!(f, OutputFormat::Video(_))));
    }
}
