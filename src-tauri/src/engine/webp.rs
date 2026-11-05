//! Adaptive lossy WebP encoder: same idea as the JPEG path, binary-searching the lowest libwebp
//! quality whose decoded result still meets a perceptual-similarity target.

use std::{fs, path::Path};

use tokio_util::sync::CancellationToken;

use super::images::{CompressionError, CompressionMode};

const MIN_QUALITY: u8 = 30;
const MAX_QUALITY: u8 = 95;

pub struct WebpOutcome {
    pub quality: u8,
    pub score: f64,
}

/// Stricter than JPEG: at the same luma score, low-quality WebP visibly boosts contrast and
/// saturation (chroma quantization), which a luma-only score does not see.
const fn target_score(mode: CompressionMode) -> f64 {
    match mode {
        CompressionMode::Quality => 0.96,
        CompressionMode::Balanced => 0.92,
        CompressionMode::Small => 0.88,
    }
}

pub fn compress_webp(
    input: &Path,
    output: &Path,
    mode: CompressionMode,
    cancel: &CancellationToken,
) -> Result<WebpOutcome, CompressionError> {
    let decoded =
        image::open(input).map_err(|error| CompressionError::Codec(error.to_string()))?;
    let rgba = decoded.to_rgba8();
    drop(decoded);
    let (width, height) = (rgba.width() as usize, rgba.height() as usize);
    let reference = luma_over_gray(rgba.as_raw());

    let encode = |quality: u8, method: i32| -> Result<Vec<u8>, CompressionError> {
        let mut config = webp::WebPConfig::new()
            .map_err(|_| CompressionError::Codec("無法初始化 WebP 編碼設定".into()))?;
        config.quality = f32::from(quality);
        config.method = method;
        config.use_sharp_yuv = 1;
        let encoded = webp::Encoder::from_rgba(rgba.as_raw(), rgba.width(), rgba.height())
            .encode_advanced(&config)
            .map_err(|error| CompressionError::Codec(format!("WebP 編碼失敗：{error:?}")))?;
        Ok(encoded.to_vec())
    };
    let measure = |bytes: &[u8]| -> Result<f64, CompressionError> {
        let candidate = image::load_from_memory_with_format(bytes, image::ImageFormat::WebP)
            .map_err(|error| CompressionError::Codec(error.to_string()))?
            .to_rgba8();
        Ok(super::similarity::plane_score(
            &reference,
            &luma_over_gray(candidate.as_raw()),
            width,
            height,
        ))
    };

    let target = target_score(mode);
    let (mut low, mut high) = (MIN_QUALITY, MAX_QUALITY);
    let mut best: Option<(u8, f64)> = None;
    while low <= high {
        if cancel.is_cancelled() {
            return Err(CompressionError::Cancelled);
        }
        let mid = low + (high - low) / 2;
        let score = measure(&encode(mid, 4)?)?;
        if score >= target {
            best = Some((mid, score));
            if mid == MIN_QUALITY {
                break;
            }
            high = mid - 1;
        } else {
            low = mid + 1;
        }
    }
    let (quality, score) = match best {
        Some(found) => found,
        None => (MAX_QUALITY, measure(&encode(MAX_QUALITY, 4)?)?),
    };

    if cancel.is_cancelled() {
        return Err(CompressionError::Cancelled);
    }
    // The slowest, smallest search level only runs once, for the chosen quality.
    fs::write(output, encode(quality, 6)?)?;
    Ok(WebpOutcome { quality, score })
}

/// Luma of each pixel composited over mid-gray, so transparency changes count as damage too.
fn luma_over_gray(rgba: &[u8]) -> Vec<u8> {
    rgba.chunks_exact(4)
        .map(|pixel| {
            let alpha = u32::from(pixel[3]);
            let blend = |value: u8| (u32::from(value) * alpha + 128 * (255 - alpha)) / 255;
            ((blend(pixel[0]) * 299 + blend(pixel[1]) * 587 + blend(pixel[2]) * 114) / 1000) as u8
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{GenericImageView, Rgba, RgbaImage};

    #[test]
    fn looser_modes_never_produce_larger_files_and_keep_alpha_and_size() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("source.webp");
        let pixels = RgbaImage::from_fn(256, 256, |x, y| {
            let noise = (x.wrapping_mul(2_654_435_761) ^ y.wrapping_mul(40_503)) % 23;
            Rgba([
                (x / 2 + noise) as u8,
                (y / 2 + noise) as u8,
                ((x + y) / 3) as u8,
                if x < 128 { 255 } else { 120 },
            ])
        });
        let encoded = webp::Encoder::from_rgba(pixels.as_raw(), 256, 256).encode(95.0);
        fs::write(&input, &*encoded).unwrap();

        let mut sizes = Vec::new();
        for mode in [CompressionMode::Quality, CompressionMode::Balanced, CompressionMode::Small] {
            let output = dir.path().join(format!("{mode:?}.webp"));
            compress_webp(&input, &output, mode, &CancellationToken::new()).unwrap();
            let decoded = image::open(&output).unwrap();
            assert_eq!(decoded.dimensions(), (256, 256));
            assert!(decoded.color().has_alpha());
            sizes.push(fs::metadata(&output).unwrap().len());
        }
        assert!(sizes[0] >= sizes[1] && sizes[1] >= sizes[2], "sizes {sizes:?}");
    }
}
