use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum PathError {
    #[error("輸入路徑必須是 JPEG 或 PNG 檔案")]
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
    if !matches!(extension.to_ascii_lowercase().as_str(), "jpg" | "jpeg" | "png") {
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
