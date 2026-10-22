use std::{io::Read, path::Path};

use image::GenericImageView;

use super::analyzer::{AnalyzeError, analyze_media};

#[derive(Debug, thiserror::Error)]
pub enum ValidationError {
    #[error(transparent)]
    Analyze(#[from] AnalyzeError),
    #[error("輸出圖片尺寸與原始檔不同")]
    DimensionsChanged,
    #[error("PNG 輸出遺失透明度")]
    TransparencyChanged,
    #[error("輸出圖片格式與原始檔格式不同")]
    FormatChanged,
}

pub fn validate_image(input: &Path, output: &Path) -> Result<u64, ValidationError> {
    let original = analyze_media(input)?;
    let output_info = analyze_media(output)?;
    if !same_format(&original.extension, &output_info.extension)
        || !matches_extension_to_signature(&output_info.extension, output)
            .map_err(AnalyzeError::Io)?
    {
        return Err(ValidationError::FormatChanged);
    }
    if (original.width, original.height) != (output_info.width, output_info.height) {
        return Err(ValidationError::DimensionsChanged);
    }
    if original.extension == "png" {
        let original_image = super::analyzer::decode_bounded(input)?;
        let output_image = super::analyzer::decode_bounded(output)?;
        let source_has_transparency = original_image.color().has_alpha()
            && original_image
                .to_rgba8()
                .pixels()
                .any(|pixel| pixel[3] < 255);
        if source_has_transparency && !output_image.color().has_alpha() {
            return Err(ValidationError::TransparencyChanged);
        }
        let (width, height) = original_image.dimensions();
        if (width, height) != (output_image.width(), output_image.height()) {
            return Err(ValidationError::DimensionsChanged);
        }
    }
    Ok(output_info.original_bytes)
}

fn same_format(left: &str, right: &str) -> bool {
    (matches!(left, "jpg" | "jpeg") && matches!(right, "jpg" | "jpeg")) || left == right
}

fn matches_extension_to_signature(extension: &str, path: &Path) -> Result<bool, std::io::Error> {
    let mut bytes = Vec::with_capacity(64);
    std::fs::File::open(path)?
        .take(64)
        .read_to_end(&mut bytes)?;
    Ok(match extension {
        "jpg" | "jpeg" => bytes.starts_with(&[0xff, 0xd8, 0xff]),
        "png" => bytes.starts_with(b"\x89PNG\r\n\x1a\n"),
        "webp" => bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP"),
        "avif" => {
            bytes.get(4..8) == Some(b"ftyp")
                && bytes.get(8..).is_some_and(|brands| {
                    brands
                        .windows(4)
                        .any(|brand| brand == b"avif" || brand == b"avis")
                })
        }
        "jxl" => {
            bytes.starts_with(&[0xff, 0x0a]) || bytes.starts_with(b"\x00\x00\x00\x0cJXL \r\n\x87\n")
        }
        _ => false,
    })
}

#[cfg(test)]
mod tests {
    use std::fs;
    use tempfile::tempdir;

    use super::{ValidationError, validate_image};

    #[test]
    fn rejects_output_with_bytes_from_another_format() {
        let dir = tempdir().unwrap();
        let jpeg = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/oriented-noise.jpg"
        );
        let input = dir.path().join("source.jpg");
        let mislabeled_output = dir.path().join("source.webp");
        fs::copy(jpeg, &input).unwrap();
        fs::copy(jpeg, &mislabeled_output).unwrap();

        assert!(matches!(
            validate_image(&input, &mislabeled_output),
            Err(ValidationError::FormatChanged)
        ));
    }
}
