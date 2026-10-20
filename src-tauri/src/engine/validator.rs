use std::path::Path;

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
}

pub fn validate_image(input: &Path, output: &Path) -> Result<u64, ValidationError> {
    let original = analyze_media(input)?;
    let output_info = analyze_media(output)?;
    if (original.width, original.height) != (output_info.width, output_info.height) {
        return Err(ValidationError::DimensionsChanged);
    }
    if original.extension == "png" {
        let original_image = super::analyzer::decode_bounded(input)?;
        let output_image = super::analyzer::decode_bounded(output)?;
        if original_image.color().has_alpha() && !output_image.color().has_alpha() {
            return Err(ValidationError::TransparencyChanged);
        }
        let (width, height) = original_image.dimensions();
        if (width, height) != (output_image.width(), output_image.height()) {
            return Err(ValidationError::DimensionsChanged);
        }
    }
    Ok(output_info.original_bytes)
}

