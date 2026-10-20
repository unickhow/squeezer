use std::{fs, path::Path};

use serde::Serialize;

pub const MAX_INPUT_BYTES: u64 = 512 * 1024 * 1024;
const MAX_IMAGE_EDGE: u32 = 20_000;
const MAX_IMAGE_PIXELS: u64 = 100_000_000;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaInfo {
    pub file_name: String,
    pub extension: String,
    pub width: u32,
    pub height: u32,
    pub original_bytes: u64,
}

#[derive(Debug, thiserror::Error)]
pub enum AnalyzeError {
    #[error("找不到檔案或無法讀取：{0}")]
    Io(#[from] std::io::Error),
    #[error("只支援 JPEG 與 PNG 圖片")]
    UnsupportedFormat,
    #[error("圖片無法解碼或檔案已損毀")]
    InvalidImage,
    #[error("圖片超過支援上限（檔案 512 MiB、單邊 20,000 像素、總計 100 百萬像素）")]
    ImageTooLarge,
}

pub fn analyze_media(path: &Path) -> Result<MediaInfo, AnalyzeError> {
    analyze_media_with_image(path).map(|(info, _)| info)
}

pub fn analyze_media_with_image(path: &Path) -> Result<(MediaInfo, image::DynamicImage), AnalyzeError> {
    let metadata = fs::metadata(path)?;
    if !metadata.is_file() {
        return Err(AnalyzeError::UnsupportedFormat);
    }
    if metadata.len() > MAX_INPUT_BYTES {
        return Err(AnalyzeError::ImageTooLarge);
    }

    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase)
        .ok_or(AnalyzeError::UnsupportedFormat)?;
    if !matches!(extension.as_str(), "jpg" | "jpeg" | "png") {
        return Err(AnalyzeError::UnsupportedFormat);
    }

    let reader = image::ImageReader::open(path)
        .map_err(|error| AnalyzeError::Io(error.into()))?
        .with_guessed_format()
        .map_err(|error| AnalyzeError::Io(error.into()))?;
    let (width, height) = reader.into_dimensions().map_err(|_| AnalyzeError::InvalidImage)?;
    if width > MAX_IMAGE_EDGE || height > MAX_IMAGE_EDGE || u64::from(width) * u64::from(height) > MAX_IMAGE_PIXELS {
        return Err(AnalyzeError::ImageTooLarge);
    }
    let decoded = decode_bounded(path)?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("Image")
        .to_string();

    Ok((MediaInfo {
        file_name,
        extension,
        width,
        height,
        original_bytes: metadata.len(),
    }, decoded))
}

pub fn decode_bounded(path: &Path) -> Result<image::DynamicImage, AnalyzeError> {
    let metadata = fs::metadata(path)?;
    if metadata.len() > MAX_INPUT_BYTES { return Err(AnalyzeError::ImageTooLarge); }
    let reader = image::ImageReader::open(path)
        .map_err(|error| AnalyzeError::Io(error.into()))?
        .with_guessed_format()
        .map_err(|error| AnalyzeError::Io(error.into()))?;
    let (width, height) = reader.into_dimensions().map_err(|_| AnalyzeError::InvalidImage)?;
    if width > MAX_IMAGE_EDGE || height > MAX_IMAGE_EDGE || u64::from(width) * u64::from(height) > MAX_IMAGE_PIXELS {
        return Err(AnalyzeError::ImageTooLarge);
    }
    let mut reader = image::ImageReader::open(path)
        .map_err(|error| AnalyzeError::Io(error.into()))?
        .with_guessed_format()
        .map_err(|error| AnalyzeError::Io(error.into()))?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_IMAGE_EDGE);
    limits.max_image_height = Some(MAX_IMAGE_EDGE);
    limits.max_alloc = Some(MAX_INPUT_BYTES);
    reader.limits(limits);
    reader.decode().map_err(|_| AnalyzeError::InvalidImage)
}

#[cfg(test)]
mod tests {
    use std::{fs, io::Cursor};

    use image::{ImageEncoder, Rgb, RgbImage, codecs::png::PngEncoder};
    use tempfile::tempdir;

    use super::{AnalyzeError, analyze_media};

    #[test]
    fn analyzes_png_dimensions_and_unicode_name() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("海邊.png");
        let mut png = Cursor::new(Vec::new());
        PngEncoder::new(&mut png)
            .write_image(&RgbImage::from_pixel(12, 7, Rgb([20, 40, 60])).into_raw(), 12, 7, image::ExtendedColorType::Rgb8)
            .unwrap();
        fs::write(&path, png.into_inner()).unwrap();

        let info = analyze_media(&path).unwrap();

        assert_eq!(info.file_name, "海邊.png");
        assert_eq!((info.width, info.height), (12, 7));
        assert_eq!(info.original_bytes, fs::metadata(path).unwrap().len());
    }

    #[test]
    fn rejects_malformed_image_without_panicking() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("broken.jpg");
        fs::write(&path, b"not an image").unwrap();

        assert!(matches!(analyze_media(&path), Err(AnalyzeError::InvalidImage)));
    }
}
