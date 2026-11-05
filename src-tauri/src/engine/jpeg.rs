//! Adaptive mozjpeg encoder.
//!
//! Instead of a fixed quality number, the encoder searches for the lowest JPEG quality whose
//! decoded result still meets a perceptual-similarity target (windowed luma SSIM). Each image
//! therefore lands right at the edge of "no visible difference" for its own content.

use std::{fs, path::Path};

use mozjpeg::{ColorSpace, Compress, Marker};
use tokio_util::sync::CancellationToken;

use super::images::{CompressionError, CompressionMode};

/// Lowest / highest quality the search will consider.
const MIN_QUALITY: u8 = 40;
const MAX_QUALITY: u8 = 95;

#[derive(Debug, Clone, Copy)]
pub struct JpegOutcome {
    pub quality: u8,
    pub score: f64,
}

/// Similarity target per mode. The score is `0.6 * mean SSIM + 0.4 * mean SSIM of the worst 10%
/// of windows`, so a few heavily damaged detail regions cannot hide behind large flat areas.
const fn target_score(mode: CompressionMode) -> f64 {
    match mode {
        CompressionMode::Quality => 0.985,
        CompressionMode::Balanced => 0.972,
        CompressionMode::Small => 0.950,
    }
}

pub fn compress_jpeg(
    input: &Path,
    output: &Path,
    mode: CompressionMode,
    preserve_metadata: bool,
    cancel: &CancellationToken,
) -> Result<JpegOutcome, CompressionError> {
    let source = fs::read(input)?;
    let layout = inspect_jpeg(&source, preserve_metadata)
        .ok_or_else(|| CompressionError::Jpeg("無法解析 JPEG 結構".into()))?;
    if layout.components == 4 {
        return Err(CompressionError::Jpeg("尚不支援 CMYK JPEG".into()));
    }
    let decoded = image::load_from_memory_with_format(&source, image::ImageFormat::Jpeg)
        .map_err(|error| CompressionError::Jpeg(error.to_string()))?;
    let (width, height) = (decoded.width() as usize, decoded.height() as usize);
    let grayscale = layout.components == 1;
    let pixels: Vec<u8> = if grayscale {
        decoded.to_luma8().into_raw()
    } else {
        decoded.to_rgb8().into_raw()
    };
    let reference = luma_plane(&pixels, grayscale);
    drop(decoded);

    let target = target_score(mode);
    let encode = |quality: u8, final_pass: bool| {
        encode_with_mozjpeg(
            &pixels,
            width,
            height,
            grayscale,
            quality,
            &layout.markers,
            final_pass,
        )
    };
    let measure = |bytes: &[u8]| -> Result<f64, CompressionError> {
        let candidate = image::load_from_memory_with_format(bytes, image::ImageFormat::Jpeg)
            .map_err(|error| CompressionError::Jpeg(error.to_string()))?;
        let candidate_pixels = if grayscale {
            candidate.to_luma8().into_raw()
        } else {
            candidate.to_rgb8().into_raw()
        };
        Ok(super::similarity::plane_score(
            &reference,
            &luma_plane(&candidate_pixels, grayscale),
            width,
            height,
        ))
    };

    // Binary search for the lowest quality that still meets the target. Quality vs. SSIM is
    // monotonic for practical purposes, so ~6 probes are enough for a 56-step range.
    let (mut low, mut high) = (MIN_QUALITY, MAX_QUALITY);
    let mut best: Option<(u8, f64)> = None;
    while low <= high {
        if cancel.is_cancelled() {
            return Err(CompressionError::Cancelled);
        }
        let mid = low + (high - low) / 2;
        let score = measure(&encode(mid, false)?)?;
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
    // If even the highest quality misses the target (noisy source), use the highest quality.
    let (quality, score) = match best {
        Some(found) => found,
        None => (MAX_QUALITY, measure(&encode(MAX_QUALITY, false)?)?),
    };

    if cancel.is_cancelled() {
        return Err(CompressionError::Cancelled);
    }
    let encoded = encode(quality, true)?;
    fs::write(output, encoded)?;
    Ok(JpegOutcome { quality, score })
}

fn encode_with_mozjpeg(
    pixels: &[u8],
    width: usize,
    height: usize,
    grayscale: bool,
    quality: u8,
    markers: &[(u8, Vec<u8>)],
    final_pass: bool,
) -> Result<Vec<u8>, CompressionError> {
    let mut compress = Compress::new(if grayscale {
        ColorSpace::JCS_GRAYSCALE
    } else {
        ColorSpace::JCS_RGB
    });
    compress.set_size(width, height);
    compress.set_quality(f32::from(quality));
    if final_pass {
        compress.set_progressive_mode();
        compress.set_optimize_scans(true);
    }
    let mut started = compress
        .start_compress(Vec::new())
        .map_err(|error| CompressionError::Jpeg(error.to_string()))?;
    for (app, data) in markers {
        started.write_marker(Marker::APP(*app), data);
    }
    started
        .write_scanlines(pixels)
        .map_err(|error| CompressionError::Jpeg(error.to_string()))?;
    started
        .finish()
        .map_err(|error| CompressionError::Jpeg(error.to_string()))
}

#[derive(Debug)]
struct JpegLayout {
    components: u8,
    /// `(APPn, payload)` segments worth carrying over (EXIF, XMP, ICC, IPTC).
    markers: Vec<(u8, Vec<u8>)>,
}

/// Walks the JPEG segment list up to the first scan.
fn inspect_jpeg(bytes: &[u8], preserve_metadata: bool) -> Option<JpegLayout> {
    if !bytes.starts_with(&[0xff, 0xd8]) {
        return None;
    }
    let mut position = 2;
    let mut components = 3;
    let mut markers = Vec::new();
    while position + 4 <= bytes.len() {
        if bytes[position] != 0xff {
            return None;
        }
        let marker = bytes[position + 1];
        if marker == 0xff {
            position += 1;
            continue;
        }
        if marker == 0xda || marker == 0xd9 {
            break;
        }
        let length = usize::from(u16::from_be_bytes([bytes[position + 2], bytes[position + 3]]));
        let end = position + 2 + length;
        if length < 2 || end > bytes.len() {
            return None;
        }
        let payload = &bytes[position + 4..end];
        match marker {
            0xc0..=0xcf if !matches!(marker, 0xc4 | 0xc8 | 0xcc) => {
                components = *payload.get(5)?;
            }
            0xe1 | 0xe2 | 0xed => {
                let keep = if marker == 0xe2 {
                    // ICC keeps colors correct; the multi-picture index would be stale.
                    !payload.starts_with(b"MPF\0")
                } else {
                    preserve_metadata
                };
                if keep {
                    markers.push((marker - 0xe0, payload.to_vec()));
                }
            }
            _ => {}
        }
        position = end;
    }
    Some(JpegLayout {
        components,
        markers,
    })
}

fn luma_plane(pixels: &[u8], grayscale: bool) -> Vec<u8> {
    if grayscale {
        return pixels.to_vec();
    }
    pixels
        .chunks_exact(3)
        .map(|rgb| {
            ((77 * u32::from(rgb[0]) + 150 * u32::from(rgb[1]) + 29 * u32::from(rgb[2])) >> 8)
                as u8
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_exif_and_icc_but_drops_multi_picture_index() {
        let mut jpeg = vec![0xff, 0xd8];
        for (marker, body) in [
            (0xe1u8, b"Exif\0\0data".to_vec()),
            (0xe2, b"ICC_PROFILE\0x".to_vec()),
            (0xe2, b"MPF\0idx".to_vec()),
        ] {
            jpeg.extend([0xff, marker]);
            jpeg.extend(((body.len() + 2) as u16).to_be_bytes());
            jpeg.extend(body);
        }
        jpeg.extend([0xff, 0xc0, 0, 11, 8, 0, 8, 0, 8, 3, 1, 0x22, 0]);
        jpeg.extend([0xff, 0xda]);

        let layout = inspect_jpeg(&jpeg, true).unwrap();
        assert_eq!(layout.components, 3);
        assert_eq!(layout.markers.len(), 2);
        let without_metadata = inspect_jpeg(&jpeg, false).unwrap();
        assert_eq!(without_metadata.markers.len(), 1);
        assert_eq!(without_metadata.markers[0].0, 2);
    }

    #[test]
    fn looser_modes_never_produce_larger_files_and_keep_dimensions() {
        let fixture = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/oriented-noise.jpg");
        let dir = tempfile::tempdir().unwrap();
        let mut sizes = Vec::new();
        for (name, mode) in [
            ("q", CompressionMode::Quality),
            ("b", CompressionMode::Balanced),
            ("s", CompressionMode::Small),
        ] {
            let output = dir.path().join(format!("{name}.jpg"));
            compress_jpeg(Path::new(fixture), &output, mode, true, &CancellationToken::new())
                .unwrap();
            sizes.push(fs::metadata(&output).unwrap().len());
            let (w, h) = image::image_dimensions(&output).unwrap();
            assert_eq!((w, h), image::image_dimensions(fixture).unwrap());
        }
        assert!(sizes[2] <= sizes[1] && sizes[1] <= sizes[0], "{sizes:?}");
    }
}
