use std::path::{Path, PathBuf};

use super::image_formats::ImageFormatKind;

#[derive(Debug, thiserror::Error)]
pub enum PathError {
    #[error("輸入路徑不是支援的圖片或影片格式")]
    InvalidInput,
    #[error("無法建立輸出檔名")]
    InvalidOutputName,
}

pub fn output_path_for(input: &Path, output_dir: &Path) -> Result<PathBuf, PathError> {
    let stem = input
        .file_stem()
        .ok_or(PathError::InvalidInput)?
        .to_string_lossy();
    let extension = input
        .extension()
        .ok_or(PathError::InvalidInput)?
        .to_string_lossy();
    if ImageFormatKind::from_extension(&extension).is_none() {
        return Err(PathError::InvalidInput);
    }

    for suffix in 1..=u32::MAX {
        let marker = if suffix == 1 {
            "squeezed".to_string()
        } else {
            format!("squeezed-{suffix}")
        };
        let candidate = output_dir.join(format!("{stem}.{marker}.{extension}"));
        if !candidate.exists() {
            return Ok(candidate);
        }
    }

    Err(PathError::InvalidOutputName)
}

pub fn video_output_path_for(input: &Path, output_dir: &Path) -> Result<PathBuf, PathError> {
    let stem = input
        .file_stem()
        .ok_or(PathError::InvalidInput)?
        .to_string_lossy();
    if !matches!(
        input
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase()
            .as_str(),
        "mp4" | "mov" | "m4v"
    ) {
        return Err(PathError::InvalidInput);
    }
    for suffix in 1..=u32::MAX {
        let marker = if suffix == 1 {
            "squeezed".to_string()
        } else {
            format!("squeezed-{suffix}")
        };
        let candidate = output_dir.join(format!("{stem}.{marker}.mp4"));
        if !candidate.exists() {
            return Ok(candidate);
        }
    }
    Err(PathError::InvalidOutputName)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use tempfile::tempdir;

    use super::{output_path_for, video_output_path_for};

    #[test]
    fn image_output_keeps_supported_image_extensions() {
        let dir = tempdir().unwrap();
        for extension in ["jpg", "jpeg", "png", "webp", "avif", "jxl"] {
            let input = dir.path().join(format!("source.{extension}"));
            fs::write(&input, b"source").unwrap();
            assert_eq!(
                output_path_for(&input, dir.path())
                    .unwrap()
                    .extension()
                    .unwrap(),
                extension
            );
        }
    }

    #[test]
    fn video_output_uses_mp4_and_increments_without_overwriting() {
        let dir = tempdir().unwrap();
        let input = dir.path().join("clip.MOV");
        fs::write(&input, b"source").unwrap();
        fs::write(dir.path().join("clip.squeezed.mp4"), b"existing").unwrap();
        assert_eq!(
            video_output_path_for(&input, dir.path())
                .unwrap()
                .file_name()
                .unwrap(),
            "clip.squeezed-2.mp4"
        );
        assert_eq!(fs::read(&input).unwrap(), b"source");
    }
}
