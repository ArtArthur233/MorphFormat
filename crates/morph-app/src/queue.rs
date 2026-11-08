//! Queue and settings state, free of any drawing code.

use morph_core::video::h264_available;
use morph_core::{
    detect_input, supported_outputs, ConvertOptions, ImageFormat, InputKind, Metadata,
    OutputFormat, Resize, VideoFormat,
};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

/// Where an item is in its life.
#[derive(Debug, Clone, PartialEq)]
pub enum Status {
    Pending,
    Running(f32),
    Done(PathBuf),
    Failed(String),
}

/// One file waiting to be converted.
#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub id: u64,
    pub path: PathBuf,
    pub kind: InputKind,
    pub status: Status,
}

/// User-chosen conversion settings.
#[derive(Debug, Clone, PartialEq)]
pub struct Settings {
    pub image_format: OutputFormat,
    pub video_format: OutputFormat,
    pub quality: u8,
    /// Shorter side in pixels (720 = 720p); `None` keeps the original size.
    pub resolution: Option<u32>,
    pub metadata: Metadata,
    /// `None` = next to the source file.
    pub out_dir: Option<PathBuf>,
}

impl Default for Settings {
    fn default() -> Self {
        let defaults = ConvertOptions::default();
        Self {
            image_format: OutputFormat::Image(ImageFormat::Jpeg),
            video_format: OutputFormat::Video(VideoFormat::Mp4H264),
            quality: defaults.quality,
            resolution: None,
            metadata: defaults.metadata,
            out_dir: None,
        }
    }
}

impl Settings {
    /// Target format for an input kind.
    pub fn target(&self, kind: InputKind) -> OutputFormat {
        match kind {
            InputKind::Image(_) => self.image_format,
            InputKind::Video => self.video_format,
        }
    }

    pub fn options(&self) -> ConvertOptions {
        let resize = self.resolution.map(Resize::ShortSide);
        ConvertOptions {
            quality: self.quality,
            resize,
            metadata: self.metadata,
            ..ConvertOptions::default()
        }
    }

    /// Folder the output of `input` goes to.
    pub fn out_dir_for(&self, input: &Path) -> PathBuf {
        self.out_dir
            .clone()
            .or_else(|| input.parent().map(Path::to_path_buf))
            .unwrap_or_default()
    }
}

/// Result of adding paths to the queue.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct AddReport {
    pub added: usize,
    pub skipped: usize,
}

/// The list of files to convert.
#[derive(Debug, Default)]
pub struct Queue {
    pub items: Vec<Item>,
    next_id: u64,
}

impl Queue {
    /// Adds files, scanning folders recursively; unsupported files are counted, not added.
    pub fn add_paths(&mut self, paths: impl IntoIterator<Item = PathBuf>) -> AddReport {
        let mut report = AddReport::default();
        for root in paths {
            for entry in WalkDir::new(&root).into_iter().filter_map(Result::ok) {
                if !entry.file_type().is_file() {
                    continue;
                }
                let path = entry.into_path();
                match detect_input(&path) {
                    Ok(kind) if !input_enabled(kind) => {
                        report.skipped += 1;
                    }
                    Ok(kind) if !self.items.iter().any(|i| i.path == path) => {
                        self.items.push(Item {
                            id: self.next_id,
                            path,
                            kind,
                            status: Status::Pending,
                        });
                        self.next_id += 1;
                        report.added += 1;
                    }
                    Ok(_) => {} // already queued
                    Err(_) => report.skipped += 1,
                }
            }
        }
        report
    }

    pub fn remove(&mut self, id: u64) {
        self.items.retain(|i| i.id != id);
    }

    /// Drops finished and failed items.
    pub fn clear_finished(&mut self) {
        self.items
            .retain(|i| !matches!(i.status, Status::Done(_) | Status::Failed(_)));
    }

    pub fn set_status(&mut self, id: u64, status: Status) {
        if let Some(i) = self.items.iter_mut().find(|i| i.id == id) {
            i.status = status;
        }
    }

    /// Ids of items to run: pending ones and previously failed ones are retried.
    pub fn runnable(&self) -> Vec<u64> {
        self.items
            .iter()
            .filter(|i| matches!(i.status, Status::Pending | Status::Failed(_)))
            .map(|i| i.id)
            .collect()
    }

    pub fn has_kind(&self, video: bool) -> bool {
        self.items
            .iter()
            .any(|i| matches!(i.kind, InputKind::Video) == video)
    }

    pub fn has_finished(&self) -> bool {
        self.items
            .iter()
            .any(|i| matches!(i.status, Status::Done(_) | Status::Failed(_)))
    }

    /// Folder of the last converted file, to open after completion.
    pub fn last_output_dir(&self) -> Option<PathBuf> {
        self.items.iter().rev().find_map(|i| match &i.status {
            Status::Done(p) => p.parent().map(Path::to_path_buf),
            _ => None,
        })
    }
}

/// Output choices for images (`video = false`) or videos, minus what this build/machine lacks.
pub fn choices(video: bool) -> Vec<OutputFormat> {
    filter_outputs(
        supported_outputs(if video {
            InputKind::Video
        } else {
            InputKind::Image(ImageFormat::Png)
        }),
        cfg!(feature = "video"),
        h264_available(),
    )
}

/// Pure part of [`choices`]: no video build -> no video formats; no H.264 encoder -> no MP4.
fn filter_outputs(all: Vec<OutputFormat>, video: bool, h264: bool) -> Vec<OutputFormat> {
    all.into_iter()
        .filter(|f| match f {
            OutputFormat::Video(VideoFormat::Mp4H264) => video && h264,
            OutputFormat::Video(_) => video,
            _ => true,
        })
        .collect()
}

/// True when this build can read the input (HEIC needs `heic`, videos need `video`).
fn input_enabled(kind: InputKind) -> bool {
    match kind {
        InputKind::Video => cfg!(feature = "video"),
        InputKind::Image(ImageFormat::Heic) => cfg!(feature = "heic"),
        InputKind::Image(_) => true,
    }
}

/// Short label of an output format.
pub fn format_label(f: OutputFormat) -> &'static str {
    match f {
        OutputFormat::Image(ImageFormat::Jpeg) => "JPEG",
        OutputFormat::Image(ImageFormat::Png) => "PNG",
        OutputFormat::Image(ImageFormat::WebP) => "WebP",
        OutputFormat::Image(ImageFormat::Gif) | OutputFormat::Video(VideoFormat::Gif) => "GIF",
        OutputFormat::Image(ImageFormat::Bmp) => "BMP",
        OutputFormat::Image(ImageFormat::Heic) => "HEIC",
        OutputFormat::Video(VideoFormat::Mp4H264) => "MP4 (H.264)",
        OutputFormat::Video(VideoFormat::WebmVp9) => "WebM (VP9)",
        OutputFormat::Video(VideoFormat::WebmAv1) => "WebM (AV1)",
        OutputFormat::Video(VideoFormat::MkvRemux) => "MKV (no re-encoding)",
    }
}

/// Short source-format label of a detected input kind ("PNG", or "Video").
pub fn kind_label(k: InputKind) -> &'static str {
    match k {
        InputKind::Image(f) => format_label(OutputFormat::Image(f)),
        InputKind::Video => "Video",
    }
}

/// True when the encoder for `f` has no quality knob (lossless, fixed palette or remux).
pub fn ignores_quality(f: OutputFormat) -> bool {
    matches!(
        f,
        OutputFormat::Image(ImageFormat::Png | ImageFormat::Bmp | ImageFormat::Gif)
            | OutputFormat::Video(VideoFormat::Gif | VideoFormat::MkvRemux)
    )
}

/// Notice shown when a batch ends, e.g. "12 converted · 1 failed".
pub fn run_summary(ok: usize, failed: usize, cancelled: bool) -> String {
    let mut s = if cancelled {
        format!("Cancelled · {ok} converted")
    } else {
        format!("{ok} converted")
    };
    if failed > 0 {
        s.push_str(&format!(" · {failed} failed"));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("morph-app-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn scans_folders_recursively_and_counts_skipped() {
        let d = scratch("scan");
        std::fs::create_dir_all(d.join("sub")).unwrap();
        std::fs::write(d.join("a.png"), b"\x89PNG\r\n\x1a\n").unwrap();
        std::fs::write(d.join("sub/b.mkv"), [0x1A, 0x45, 0xDF, 0xA3]).unwrap();
        std::fs::write(d.join("sub/notes.txt"), b"hi").unwrap();
        let mut q = Queue::default();
        let r = q.add_paths([d.clone()]);
        assert_eq!(
            r,
            // Without the `video` feature the .mkv is skipped too.
            if cfg!(feature = "video") {
                AddReport {
                    added: 2,
                    skipped: 1,
                }
            } else {
                AddReport {
                    added: 1,
                    skipped: 2,
                }
            }
        );
        assert_eq!(q.has_kind(true), cfg!(feature = "video"));
        assert!(q.has_kind(false));
        // Adding the same file again queues nothing.
        assert_eq!(q.add_paths([d.join("a.png")]).added, 0);
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn remove_clear_and_runnable() {
        let mut q = Queue::default();
        for n in 0..3 {
            q.items.push(Item {
                id: n,
                path: format!("{n}.png").into(),
                kind: InputKind::Image(ImageFormat::Png),
                status: Status::Pending,
            });
        }
        q.set_status(0, Status::Done("o/0.jpg".into()));
        q.set_status(1, Status::Failed("x".into()));
        assert_eq!(q.runnable(), vec![1, 2]);
        assert_eq!(q.last_output_dir(), Some(PathBuf::from("o")));
        q.clear_finished();
        assert_eq!(q.items.len(), 1);
        q.remove(2);
        assert!(q.items.is_empty() && !q.has_finished());
    }

    #[test]
    fn settings_map_to_options() {
        let mut s = Settings::default();
        assert_eq!(s.options().metadata, Metadata::Strip);
        assert_eq!(s.options().resize, None);
        s.resolution = Some(720);
        s.quality = 70;
        let o = s.options();
        assert_eq!(o.quality, 70);
        assert_eq!(o.resize, Some(Resize::ShortSide(720)));
        assert_eq!(s.out_dir_for(Path::new("/x/a.png")), PathBuf::from("/x"));
        s.out_dir = Some("/out".into());
        assert_eq!(s.out_dir_for(Path::new("/x/a.png")), PathBuf::from("/out"));
        assert_eq!(s.target(InputKind::Video), s.video_format);
    }

    #[test]
    fn default_formats_are_offered() {
        let s = Settings::default();
        assert!(choices(false).contains(&s.image_format));
    }

    #[test]
    fn unavailable_video_outputs_are_hidden() {
        let all = supported_outputs(InputKind::Video);
        let mp4 = OutputFormat::Video(VideoFormat::Mp4H264);
        assert!(filter_outputs(all.clone(), false, true).is_empty());
        let no_h264 = filter_outputs(all.clone(), true, false);
        assert!(!no_h264.contains(&mp4) && no_h264.len() == all.len() - 1);
        assert_eq!(filter_outputs(all.clone(), true, true), all);
    }

    #[test]
    fn quality_and_summary() {
        assert!(ignores_quality(OutputFormat::Image(ImageFormat::Png)));
        assert!(ignores_quality(OutputFormat::Video(VideoFormat::MkvRemux)));
        assert!(!ignores_quality(OutputFormat::Image(ImageFormat::Jpeg)));
        assert!(!ignores_quality(OutputFormat::Image(ImageFormat::WebP)));
        assert!(!ignores_quality(OutputFormat::Video(VideoFormat::WebmVp9)));
        assert_eq!(run_summary(12, 1, false), "12 converted · 1 failed");
        assert_eq!(run_summary(5, 0, true), "Cancelled · 5 converted");
        assert_eq!(kind_label(InputKind::Image(ImageFormat::Png)), "PNG");
    }

    #[test]
    fn disabled_inputs_are_skipped() {
        assert!(input_enabled(InputKind::Image(ImageFormat::Png)));
        assert_eq!(
            input_enabled(InputKind::Image(ImageFormat::Heic)),
            cfg!(feature = "heic")
        );
        assert_eq!(input_enabled(InputKind::Video), cfg!(feature = "video"));
    }
}
