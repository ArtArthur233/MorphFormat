//! Output path computation.

use crate::types::OutputFormat;
use std::path::{Path, PathBuf};

/// Output path for `input` in `out_dir`, never an existing file (adds " (1)", " (2)"...).
pub fn output_path(input: &Path, format: OutputFormat, out_dir: &Path) -> PathBuf {
    let stem = input
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "output".into());
    let ext = format.extension();
    let mut candidate = out_dir.join(format!("{stem}.{ext}"));
    let mut n = 1;
    // The input itself exists, so converting in place never overwrites it.
    while candidate.exists() {
        candidate = out_dir.join(format!("{stem} ({n}).{ext}"));
        n += 1;
    }
    candidate
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ImageFormat;

    #[test]
    fn never_returns_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("a.png");
        let fmt = OutputFormat::Image(ImageFormat::Jpeg);
        let first = output_path(&input, fmt, dir.path());
        assert_eq!(first, dir.path().join("a.jpg"));
        std::fs::write(&first, b"x").unwrap();
        let second = output_path(&input, fmt, dir.path());
        assert_eq!(second, dir.path().join("a (1).jpg"));
        std::fs::write(&second, b"x").unwrap();
        assert_eq!(
            output_path(&input, fmt, dir.path()),
            dir.path().join("a (2).jpg")
        );
    }

    #[test]
    fn same_extension_does_not_hit_input() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("a.png");
        std::fs::write(&input, b"x").unwrap();
        let out = output_path(&input, OutputFormat::Image(ImageFormat::Png), dir.path());
        assert_eq!(out, dir.path().join("a (1).png"));
    }
}
