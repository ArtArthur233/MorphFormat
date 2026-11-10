//! Without the `heic` feature, HEIC input fails with a clear error.
#![cfg(not(feature = "heic"))]

use morph_core::{convert, ConvertOptions, Error, ImageFormat, Job, OutputFormat};
use std::sync::atomic::AtomicBool;

#[test]
fn heic_input_reports_feature_disabled() {
    let dir = tempfile::tempdir().unwrap();
    let job = Job {
        input: concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/heic/alpha.heif"
        )
        .into(),
        output: dir.path().join("out.png"),
        format: OutputFormat::Image(ImageFormat::Png),
        options: ConvertOptions::default(),
    };
    let err = convert(&job, &|_| {}, &AtomicBool::new(false)).unwrap_err();
    assert!(matches!(err, Error::FeatureDisabled("HEIC")), "{err:?}");
    assert_eq!(err.to_string(), "HEIC support not enabled in this build");
    assert!(!job.output.exists());
}
