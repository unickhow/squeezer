use std::{io::Cursor, path::Path};

use base64::{Engine, engine::general_purpose::STANDARD};

use super::image_formats::ImageFormatKind;

#[derive(Debug, thiserror::Error)]
pub enum PreviewError {
    #[error("無法讀取圖片")]
    Read(#[from] std::io::Error),
    #[error("不支援此圖片格式的預覽")]
    UnsupportedFormat,
    #[error("圖片無法解碼")]
    InvalidImage,
    #[error("無法建立預覽")]
    Encode,
}

pub fn image_preview(path: &Path) -> Result<String, PreviewError> {
    render_preview(path, 960, 720)
}

fn render_preview(path: &Path, max_width: u32, max_height: u32) -> Result<String, PreviewError> {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .ok_or(PreviewError::UnsupportedFormat)?;
    let format = image_format(extension)?;
    let image = super::analyzer::decode_bounded(path).map_err(|error| match error {
        super::analyzer::AnalyzeError::Io(error) => PreviewError::Read(error),
        _ => PreviewError::InvalidImage,
    })?;
    encode_image(&image, format, max_width, max_height)
}

pub fn image_thumbnail_from_image(
    image: &image::DynamicImage,
    format: image::ImageFormat,
) -> Result<String, PreviewError> {
    encode_image(image, format, 160, 120)
}

fn image_format(extension: &str) -> Result<image::ImageFormat, PreviewError> {
    ImageFormatKind::from_extension(extension)
        .map(ImageFormatKind::preview_format)
        .ok_or(PreviewError::UnsupportedFormat)
}

fn encode_image(
    image: &image::DynamicImage,
    format: image::ImageFormat,
    max_width: u32,
    max_height: u32,
) -> Result<String, PreviewError> {
    let thumbnail = image.thumbnail(max_width, max_height);
    let mut encoded = Cursor::new(Vec::new());
    thumbnail
        .write_to(&mut encoded, format)
        .map_err(|_| PreviewError::Encode)?;
    let mime = format.to_mime_type();
    Ok(format!(
        "data:{mime};base64,{}",
        STANDARD.encode(encoded.into_inner())
    ))
}

#[cfg(test)]
mod tests {
    use super::{PreviewError, image_preview, image_thumbnail_from_image};
    use image::{ImageEncoder, Rgb, RgbImage, codecs::png::PngEncoder};
    use std::{fs, io::Cursor, path::Path};
    use tempfile::tempdir;

    #[test]
    fn returns_bounded_local_png_preview() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("preview.png");
        let mut png = Cursor::new(Vec::new());
        PngEncoder::new(&mut png)
            .write_image(
                &RgbImage::from_pixel(1200, 900, Rgb([10, 20, 30])).into_raw(),
                1200,
                900,
                image::ExtendedColorType::Rgb8,
            )
            .unwrap();
        fs::write(&path, png.into_inner()).unwrap();

        let preview = image_preview(&path).unwrap();
        assert!(preview.starts_with("data:image/png;base64,"));
        let encoded = &preview["data:image/png;base64,".len()..];
        let bytes =
            base64::Engine::decode(&base64::engine::general_purpose::STANDARD, encoded).unwrap();
        let image = image::load_from_memory(&bytes).unwrap();
        assert!(image.width() <= 960 && image.height() <= 720);
    }

    #[test]
    fn returns_small_local_thumbnail_for_queue_rows() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("thumbnail.png");
        let mut png = Cursor::new(Vec::new());
        PngEncoder::new(&mut png)
            .write_image(
                &RgbImage::from_pixel(1200, 900, Rgb([10, 20, 30])).into_raw(),
                1200,
                900,
                image::ExtendedColorType::Rgb8,
            )
            .unwrap();
        fs::write(&path, png.into_inner()).unwrap();

        let image = super::super::analyzer::decode_bounded(&path).unwrap();
        let preview = image_thumbnail_from_image(&image, image::ImageFormat::Png).unwrap();
        assert!(preview.starts_with("data:image/png;base64,"));
        let encoded = &preview["data:image/png;base64,".len()..];
        let bytes =
            base64::Engine::decode(&base64::engine::general_purpose::STANDARD, encoded).unwrap();
        let image = image::load_from_memory(&bytes).unwrap();
        assert!(image.width() <= 160 && image.height() <= 120);
    }

    #[test]
    fn rejects_unsupported_preview_paths() {
        assert!(matches!(
            image_preview(Path::new("movie.mov")),
            Err(PreviewError::UnsupportedFormat)
        ));
    }
}
