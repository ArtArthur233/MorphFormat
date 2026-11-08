//! Public data types of the conversion API.

use std::path::PathBuf;

/// Raster image format (`Heic` is input only).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ImageFormat {
    Jpeg,
    Png,
    WebP,
    Gif,
    Bmp,
    Heic,
}

impl ImageFormat {
    /// File extension used for outputs, without the dot.
    pub fn extension(self) -> &'static str {
        match self {
            Self::Jpeg => "jpg",
            Self::Png => "png",
            Self::WebP => "webp",
            Self::Gif => "gif",
            Self::Bmp => "bmp",
            Self::Heic => "heic",
        }
    }
}

/// Video output format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VideoFormat {
    Mp4H264,
    WebmVp9,
    WebmAv1,
    MkvRemux,
    Gif,
}

impl VideoFormat {
    /// File extension used for outputs, without the dot.
    pub fn extension(self) -> &'static str {
        match self {
            Self::Mp4H264 => "mp4",
            Self::WebmVp9 | Self::WebmAv1 => "webm",
            Self::MkvRemux => "mkv",
            Self::Gif => "gif",
        }
    }
}

/// A target format, image or video.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OutputFormat {
    Image(ImageFormat),
    Video(VideoFormat),
}

impl OutputFormat {
    /// File extension used for outputs, without the dot.
    pub fn extension(self) -> &'static str {
        match self {
            Self::Image(f) => f.extension(),
            Self::Video(f) => f.extension(),
        }
    }
}

/// What kind of file an input is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InputKind {
    Image(ImageFormat),
    Video,
}

/// How to resize the picture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resize {
    /// Scale to this percentage of the original size.
    Percent(u8),
    /// Fit within the box, keeping the aspect ratio.
    Fit { max_w: u32, max_h: u32 },
    /// Shorter side at most this many pixels (720 = 720p, also for portrait), never upscaled.
    ShortSide(u32),
}

/// Whether to keep or drop metadata (EXIF, location...).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Metadata {
    Keep,
    Strip,
}

/// Tunable conversion settings.
#[derive(Debug, Clone, PartialEq)]
pub struct ConvertOptions {
    /// Quality from 1 to 100.
    pub quality: u8,
    /// Optional resize.
    pub resize: Option<Resize>,
    /// Metadata policy.
    pub metadata: Metadata,
    /// Video bitrate in kbit/s, overriding `quality` when set.
    pub video_bitrate_kbps: Option<u32>,
}

impl Default for ConvertOptions {
    /// Quality 90, no resize, metadata stripped (privacy first).
    fn default() -> Self {
        Self {
            quality: 90,
            resize: None,
            metadata: Metadata::Strip,
            video_bitrate_kbps: None,
        }
    }
}

/// One conversion to run.
#[derive(Debug, Clone, PartialEq)]
pub struct Job {
    /// Source file.
    pub input: PathBuf,
    /// Destination file (must not exist; see `output_path`).
    pub output: PathBuf,
    /// Target format.
    pub format: OutputFormat,
    /// Settings.
    pub options: ConvertOptions,
}
