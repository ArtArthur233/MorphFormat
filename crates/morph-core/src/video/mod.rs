//! Video engine (FFmpeg through `ffmpeg-next`, feature `video`).
//!
//! FFmpeg is linked dynamically and must be an LGPL build (see `docs/build-ffmpeg.md`).

#[cfg(feature = "video")]
mod engine;

use crate::{Job, Result};
use std::sync::atomic::AtomicBool;

/// Converts a video job; FFmpeg does the decoding, encoding and muxing.
#[cfg(feature = "video")]
pub fn convert(job: &Job, progress: &dyn Fn(f32), cancel: &AtomicBool) -> Result<()> {
    engine::convert(job, progress, cancel)
}

/// Converts a video job; this build has no `video` feature.
#[cfg(not(feature = "video"))]
pub fn convert(_job: &Job, _progress: &dyn Fn(f32), _cancel: &AtomicBool) -> Result<()> {
    Err(crate::Error::FeatureDisabled("video"))
}

/// True when a system H.264 encoder (Media Foundation, VideoToolbox, V4L2) is usable here.
/// The UI should hide the MP4 option when it returns false.
#[cfg(feature = "video")]
pub fn h264_available() -> bool {
    engine::h264_available()
}

/// No `video` feature: no encoder at all.
#[cfg(not(feature = "video"))]
pub fn h264_available() -> bool {
    false
}
