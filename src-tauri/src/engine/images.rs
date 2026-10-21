use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::Duration,
};

use tokio_util::sync::CancellationToken;

pub use super::jobs::{CompressionJob, JobStatus, MediaType};
use super::{paths::output_path_for, validator::validate_image};

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CompressionMode {
    Quality,
    Balanced,
    Small,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompressionOptions {
    pub mode: CompressionMode,
    pub output_format: String,
    pub preserve_metadata: bool,
    pub min_savings_percent: u8,
}

impl Default for CompressionOptions {
    fn default() -> Self {
        Self {
            mode: CompressionMode::Quality,
            output_format: "original".into(),
            preserve_metadata: true,
            min_savings_percent: 10,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum CompressionError {
    #[error("工作已取消")]
    Cancelled,
    #[error("圖片分析失敗：{0}")]
    Analyze(#[from] super::analyzer::AnalyzeError),
    #[error("路徑無效：{0}")]
    Path(#[from] super::paths::PathError),
    #[error("PNG 最佳化失敗：{0}")]
    Png(#[from] oxipng::PngError),
    #[error("JPEG 壓縮失敗：{0}")]
    Jpeg(String),
    #[error("圖片編碼失敗：{0}")]
    Codec(String),
    #[error("PNG 色彩縮減失敗：{0}")]
    PngQuantize(String),
    #[error("輸出驗證失敗：{0}")]
    Validation(#[from] super::validator::ValidationError),
    #[error("無法安全建立輸出檔：{0}")]
    Output(#[from] std::io::Error),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompressionResult {
    pub status: JobStatus,
    pub output_path: Option<PathBuf>,
    pub output_bytes: Option<u64>,
    pub skip_reason: Option<String>,
}

pub fn compress_image<F>(
    job: &CompressionJob,
    options: &CompressionOptions,
    cancel: &CancellationToken,
    on_validating: F,
) -> Result<CompressionResult, CompressionError>
where
    F: FnOnce(),
{
    if cancel.is_cancelled() {
        return Err(CompressionError::Cancelled);
    }
    let input = Path::new(&job.input_path);
    let output_dir = input.parent().unwrap_or_else(|| Path::new("."));
    let final_path = output_path_for(input, output_dir)?;
    let temp_path = temporary_path(&final_path, &job.id);
    let _temp_guard = TempOutput(temp_path.clone());

    if job.extension.eq_ignore_ascii_case("png") {
        if fs::metadata(input)?.len() > super::analyzer::MAX_INPUT_BYTES {
            return Err(super::analyzer::AnalyzeError::ImageTooLarge.into());
        }
        let source = fs::read(input)?;
        if is_animated_png(&source) {
            return Err(CompressionError::Codec(
                "APNG 動畫保留壓縮器尚未就緒".into(),
            ));
        }
        let preset = match options.mode {
            CompressionMode::Quality => 2,
            CompressionMode::Balanced => 4,
            CompressionMode::Small => 6,
        };
        let indexed = quantize_png(&source)
            .map_err(|error| CompressionError::PngQuantize(error.to_string()))?;
        let optimized =
            oxipng::optimize_from_memory(&indexed, &oxipng::Options::from_preset(preset))?;
        if cancel.is_cancelled() {
            return Err(CompressionError::Cancelled);
        }
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp_path)?;
        file.write_all(&optimized)?;
        file.sync_all()?;
    } else if job.extension.eq_ignore_ascii_case("webp") {
        if fs::metadata(input)?.len() > super::analyzer::MAX_INPUT_BYTES {
            return Err(super::analyzer::AnalyzeError::ImageTooLarge.into());
        }
        let decoded =
            image::open(input).map_err(|error| CompressionError::Codec(error.to_string()))?;
        let rgba = decoded.to_rgba8();
        let quality = match options.mode {
            CompressionMode::Quality => 90.0,
            CompressionMode::Balanced => 82.0,
            CompressionMode::Small => 68.0,
        };
        let encoded =
            webp::Encoder::from_rgba(rgba.as_raw(), rgba.width(), rgba.height()).encode(quality);
        if cancel.is_cancelled() {
            return Err(CompressionError::Cancelled);
        }
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp_path)?;
        file.write_all(encoded.as_ref())?;
        file.sync_all()?;
    } else if job.extension.eq_ignore_ascii_case("jpg")
        || job.extension.eq_ignore_ascii_case("jpeg")
    {
        let quality = match options.mode {
            CompressionMode::Quality => "90",
            CompressionMode::Balanced => "82",
            CompressionMode::Small => "68",
        };
        run_sips(input, &temp_path, quality, cancel)?;
    } else {
        return Err(CompressionError::Codec(format!(
            "{} 格式的壓縮器尚未就緒",
            job.extension.to_ascii_uppercase()
        )));
    }

    if cancel.is_cancelled() {
        return Err(CompressionError::Cancelled);
    }
    on_validating();
    let output_bytes = validate_image(input, &temp_path)?;
    if cancel.is_cancelled() {
        return Err(CompressionError::Cancelled);
    }
    let savings_percent = job
        .original_bytes
        .saturating_sub(output_bytes)
        .saturating_mul(100)
        / job.original_bytes.max(1);
    if output_bytes >= job.original_bytes
        || savings_percent < u64::from(options.min_savings_percent)
    {
        return Ok(CompressionResult {
            status: JobStatus::Skipped,
            output_path: None,
            output_bytes: None,
            skip_reason: Some(format!(
                "節省未達設定門檻（{}%）。",
                options.min_savings_percent
            )),
        });
    }

    let committed_path = commit_without_overwrite(&temp_path, input, output_dir)?;
    Ok(CompressionResult {
        status: JobStatus::Completed,
        output_path: Some(committed_path.clone()),
        output_bytes: Some(fs::metadata(committed_path)?.len()),
        skip_reason: None,
    })
}

fn quantize_png(source: &[u8]) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let image = image::load_from_memory_with_format(source, image::ImageFormat::Png)?.to_rgba8();
    if image.pixels().all(|pixel| pixel[3] == 255) {
        return quantize_opaque_png(&image);
    }
    let mut histogram = [0u64; 2048];
    for pixel in image.pixels() {
        histogram[palette_bin(pixel.0)] += 1;
    }
    let mut boxes = vec![
        (0usize..2048)
            .filter(|index| histogram[*index] > 0)
            .collect::<Vec<_>>(),
    ];
    while boxes.len() < 256 {
        let candidate = boxes
            .iter()
            .enumerate()
            .filter(|(_, values)| values.len() > 1)
            .max_by_key(|(_, values)| values.iter().map(|index| histogram[*index]).sum::<u64>())
            .map(|(index, _)| index);
        let Some(candidate) = candidate else { break };
        let mut values = boxes.remove(candidate);
        let channel = (0..4)
            .max_by_key(|channel| {
                let (low, high) = values
                    .iter()
                    .map(|index| bin_channels(*index)[*channel])
                    .fold((u8::MAX, 0), |(lo, hi), value| {
                        (lo.min(value), hi.max(value))
                    });
                high - low
            })
            .unwrap();
        values.sort_by_key(|index| bin_channels(*index)[channel]);
        let total: u64 = values.iter().map(|index| histogram[*index]).sum();
        let mut accumulated = 0;
        let split = values
            .iter()
            .position(|index| {
                accumulated += histogram[*index];
                accumulated >= total.div_ceil(2)
            })
            .unwrap_or(values.len() / 2)
            .clamp(1, values.len() - 1);
        let right = values.split_off(split);
        boxes.push(values);
        boxes.push(right);
    }
    let palette: Vec<[u8; 4]> = boxes
        .iter()
        .map(|values| {
            let mut sums = [0u64; 4];
            let mut total = 0u64;
            for index in values {
                let count = histogram[*index];
                let channels = bin_channels(*index);
                total += count;
                for channel in 0..4 {
                    sums[channel] += u64::from(channels[channel]) * count;
                }
            }
            std::array::from_fn(|channel| (sums[channel] / total) as u8)
        })
        .collect();
    let indices = map_to_palette(&image, &palette, true);
    encode_indexed_png(image.width(), image.height(), &palette, &indices)
}

fn quantize_opaque_png(image: &image::RgbaImage) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    use std::collections::HashMap;

    use quantette::{ImageBuf, Pipeline, QuantizeMethod};

    let rgb = image::RgbImage::from_fn(image.width(), image.height(), |x, y| {
        let pixel = image.get_pixel(x, y);
        image::Rgb([pixel[0], pixel[1], pixel[2]])
    });
    let input = ImageBuf::try_from(rgb)?;
    let quantized = Pipeline::new()
        .palette_size(256u16.try_into()?)
        .quantize_method(QuantizeMethod::kmeans())
        .input_image(input.as_ref())
        .output_srgb8_image();

    let mut palette_indices = HashMap::new();
    let mut colors = Vec::new();
    let indices = quantized
        .as_slice()
        .iter()
        .map(|pixel| {
            let color = [pixel.red, pixel.green, pixel.blue];
            *palette_indices.entry(color).or_insert_with(|| {
                let index = colors.len() as u8;
                colors.push(color);
                index
            })
        })
        .collect::<Vec<_>>();
    let palette = colors
        .iter()
        .map(|color| [color[0], color[1], color[2], 255])
        .collect::<Vec<_>>();

    encode_indexed_png(image.width(), image.height(), &palette, &indices)
}

fn encode_indexed_png(
    width: u32,
    height: u32,
    palette: &[[u8; 4]],
    indices: &[u8],
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut output = Vec::new();
    let mut encoder = png::Encoder::new(&mut output, width, height);
    encoder.set_color(png::ColorType::Indexed);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_palette(
        palette
            .iter()
            .flat_map(|color| color[..3].iter().copied())
            .collect::<Vec<_>>(),
    );
    encoder.set_trns(palette.iter().map(|color| color[3]).collect::<Vec<_>>());
    let mut writer = encoder.write_header()?;
    writer.write_image_data(&indices)?;
    writer.finish()?;
    Ok(output)
}

fn map_to_palette(image: &image::RgbaImage, palette: &[[u8; 4]], dither: bool) -> Vec<u8> {
    let width = image.width() as usize;
    let height = image.height() as usize;
    let mut indices = Vec::with_capacity(width * height);
    let mut current_errors = vec![[0.0f32; 4]; width + 2];
    let mut next_errors = vec![[0.0f32; 4]; width + 2];

    for y in 0..height {
        for x in 0..width {
            let pixel = image.get_pixel(x as u32, y as u32).0;
            let adjusted = std::array::from_fn(|channel| {
                (f32::from(pixel[channel]) + current_errors[x + 1][channel]).clamp(0.0, 255.0)
            });
            let selected = palette
                .iter()
                .enumerate()
                .min_by(|(_, a), (_, b)| {
                    palette_distance(adjusted, **a).total_cmp(&palette_distance(adjusted, **b))
                })
                .map(|(index, _)| index)
                .unwrap_or(0);
            indices.push(selected as u8);

            if dither {
                let chosen = palette[selected];
                let error: [f32; 4] =
                    std::array::from_fn(|channel| adjusted[channel] - f32::from(chosen[channel]));
                for channel in 0..4 {
                    current_errors[x + 2][channel] += error[channel] * (7.0 / 16.0);
                    next_errors[x][channel] += error[channel] * (3.0 / 16.0);
                    next_errors[x + 1][channel] += error[channel] * (5.0 / 16.0);
                    next_errors[x + 2][channel] += error[channel] * (1.0 / 16.0);
                }
            }
        }
        std::mem::swap(&mut current_errors, &mut next_errors);
        next_errors.fill([0.0; 4]);
    }
    indices
}

fn palette_distance(pixel: [f32; 4], color: [u8; 4]) -> f32 {
    let red = pixel[0] - f32::from(color[0]);
    let green = pixel[1] - f32::from(color[1]);
    let blue = pixel[2] - f32::from(color[2]);
    let alpha = pixel[3] - f32::from(color[3]);
    0.299 * red * red + 0.587 * green * green + 0.114 * blue * blue + 0.8 * alpha * alpha
}

fn is_animated_png(source: &[u8]) -> bool {
    if !source.starts_with(b"\x89PNG\r\n\x1a\n") {
        return false;
    }
    let mut offset = 8usize;
    while offset
        .checked_add(12)
        .is_some_and(|end| end <= source.len())
    {
        let chunk_length =
            u32::from_be_bytes(source[offset..offset + 4].try_into().unwrap()) as usize;
        let Some(chunk_end) = offset
            .checked_add(12)
            .and_then(|header_end| header_end.checked_add(chunk_length))
        else {
            return false;
        };
        if chunk_end > source.len() {
            return false;
        }
        if &source[offset + 4..offset + 8] == b"acTL" {
            return true;
        }
        if &source[offset + 4..offset + 8] == b"IEND" {
            return false;
        }
        offset = chunk_end;
    }
    false
}

fn palette_bin([red, green, blue, alpha]: [u8; 4]) -> usize {
    usize::from(red >> 5) * 256
        + usize::from(green >> 5) * 32
        + usize::from(blue >> 5) * 4
        + usize::from(alpha >> 6)
}

fn bin_channels(index: usize) -> [u8; 4] {
    [
        (((index >> 8) & 7) * 255 / 7) as u8,
        (((index >> 5) & 7) * 255 / 7) as u8,
        (((index >> 2) & 7) * 255 / 7) as u8,
        ((index & 3) * 255 / 3) as u8,
    ]
}

fn temporary_path(output: &Path, job_id: &str) -> PathBuf {
    let stem = output.file_stem().unwrap_or_default().to_string_lossy();
    let extension = output.extension().unwrap_or_default().to_string_lossy();
    output.with_file_name(format!(".{stem}.{job_id}.tmp.{extension}"))
}

fn run_sips(
    input: &Path,
    output: &Path,
    quality: &str,
    cancel: &CancellationToken,
) -> Result<(), CompressionError> {
    let mut child = Command::new("/usr/bin/sips")
        .args(["-s", "format", "jpeg", "-s", "formatOptions", quality, "-o"])
        .arg(output)
        .arg(input)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| {
            CompressionError::Jpeg(format!("無法啟動 macOS ImageIO（sips）：{error}"))
        })?;
    loop {
        if cancel.is_cancelled() {
            let _ = child.kill();
            let _ = child.wait();
            return Err(CompressionError::Cancelled);
        }
        if let Some(status) = child.try_wait()? {
            if status.success() {
                return Ok(());
            }
            let stderr = child
                .wait_with_output()
                .map(|output| String::from_utf8_lossy(&output.stderr).trim().to_owned())
                .unwrap_or_default();
            return Err(CompressionError::Jpeg(if stderr.is_empty() {
                "sips 無法編碼此圖片".into()
            } else {
                stderr
            }));
        }
        thread::sleep(Duration::from_millis(40));
    }
}

fn commit_without_overwrite(
    temp: &Path,
    input: &Path,
    output_dir: &Path,
) -> Result<PathBuf, CompressionError> {
    for _ in 0..100 {
        let candidate = output_path_for(input, output_dir)?;
        match fs::hard_link(temp, &candidate) {
            Ok(()) => {
                fs::remove_file(temp)?;
                return Ok(candidate);
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(CompressionError::Output(error)),
        }
    }
    Err(CompressionError::Jpeg(
        "無法安全決定輸出檔名；請清理重複檔案後重試".into(),
    ))
}

struct TempOutput(PathBuf);
impl Drop for TempOutput {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CompressionJob, CompressionMode, CompressionOptions, JobStatus, MediaType, compress_image,
    };
    use image::{GenericImageView, ImageEncoder, Rgba, RgbaImage, codecs::png::PngEncoder};
    use std::{fs, io::Cursor};
    use tempfile::tempdir;
    use tokio_util::sync::CancellationToken;

    fn make_png(path: &std::path::Path) {
        let mut encoded = Cursor::new(Vec::new());
        PngEncoder::new(&mut encoded)
            .write_image(
                &RgbaImage::from_pixel(128, 96, Rgba([40, 120, 210, 128])).into_raw(),
                128,
                96,
                image::ExtendedColorType::Rgba8,
            )
            .unwrap();
        fs::write(path, encoded.into_inner()).unwrap();
    }
    fn job(path: &std::path::Path) -> CompressionJob {
        let info = super::super::analyzer::analyze_media(path).unwrap();
        CompressionJob {
            id: "test-job".into(),
            input_path: path.to_string_lossy().into(),
            output_path: None,
            thumbnail: None,
            file_name: info.file_name,
            extension: info.extension,
            media_type: MediaType::Image,
            width: info.width,
            height: info.height,
            duration_ms: None,
            codec: None,
            audio_codec: None,
            status: JobStatus::Queued,
            progress: 0,
            original_bytes: info.original_bytes,
            output_bytes: None,
            error: None,
            skip_reason: None,
        }
    }

    #[test]
    fn png_optimization_keeps_alpha_and_never_changes_source() {
        let dir = tempdir().unwrap();
        let input = dir.path().join("透明.png");
        make_png(&input);
        let original = fs::read(&input).unwrap();
        let result = compress_image(
            &job(&input),
            &CompressionOptions::default(),
            &CancellationToken::new(),
            || {},
        )
        .unwrap();
        assert_eq!(fs::read(&input).unwrap(), original);
        if let Some(path) = result.output_path {
            let image = image::open(path).unwrap();
            assert!(image.color().has_alpha());
            assert_eq!(image.dimensions(), (128, 96));
        } else {
            assert_eq!(result.status, JobStatus::Skipped);
        }
        assert_eq!(result.status, JobStatus::Completed);
    }

    #[test]
    fn png_optimization_automatically_quantizes_colors_and_preserves_transparency() {
        let dir = tempdir().unwrap();
        let input = dir.path().join("colors.png");
        let mut pixels = RgbaImage::new(256, 128);
        for (x, y, pixel) in pixels.enumerate_pixels_mut() {
            *pixel = Rgba([
                x as u8,
                y as u8,
                (x.wrapping_mul(y) % 256) as u8,
                if x < 128 { 255 } else { 96 },
            ]);
        }
        let mut encoded = Cursor::new(Vec::new());
        PngEncoder::new(&mut encoded)
            .write_image(
                &pixels.into_raw(),
                256,
                128,
                image::ExtendedColorType::Rgba8,
            )
            .unwrap();
        fs::write(&input, encoded.into_inner()).unwrap();
        let original = fs::read(&input).unwrap();

        let options = CompressionOptions {
            min_savings_percent: 0,
            ..Default::default()
        };
        let result =
            compress_image(&job(&input), &options, &CancellationToken::new(), || {}).unwrap();

        assert_eq!(fs::read(&input).unwrap(), original);
        let output = result
            .output_path
            .expect("color reduction should make this fixture smaller");
        let bytes = fs::read(output).unwrap();
        let decoder = png::Decoder::new(Cursor::new(&bytes));
        let reader = decoder.read_info().unwrap();
        assert_eq!(reader.info().color_type, png::ColorType::Indexed);
        assert!(
            reader
                .info()
                .trns
                .as_ref()
                .is_some_and(|alpha| alpha.iter().any(|value| *value < 255))
        );
        assert!(bytes.len() < original.len());
    }

    #[test]
    fn png_palette_mapping_dithers_gradients_without_changing_average_tone() {
        let mut gradient = RgbaImage::new(64, 64);
        for (x, _, pixel) in gradient.enumerate_pixels_mut() {
            let value = (x * 4) as u8;
            *pixel = Rgba([value, value, value, 255]);
        }
        let palette = [[0, 0, 0, 255], [128, 128, 128, 255], [252, 252, 252, 255]];

        let plain = super::map_to_palette(&gradient, &palette, false);
        let dithered = super::map_to_palette(&gradient, &palette, true);
        let mean_tone = |indices: &[u8]| {
            indices
                .iter()
                .map(|index| f64::from(palette[*index as usize][0]))
                .sum::<f64>()
                / indices.len() as f64
        };

        assert_ne!(dithered, plain);
        assert!((mean_tone(&dithered) - mean_tone(&plain)).abs() < 5.0);
    }

    #[test]
    fn opaque_png_uses_perceptual_dithered_palette_with_bounded_color_error() {
        let mut source = RgbaImage::new(128, 64);
        for (x, y, pixel) in source.enumerate_pixels_mut() {
            *pixel = Rgba([
                (x * 2) as u8,
                (y * 4) as u8,
                (x.wrapping_mul(y) % 256) as u8,
                255,
            ]);
        }
        let encoded = super::quantize_opaque_png(&source).unwrap();
        let optimized = image::load_from_memory_with_format(&encoded, image::ImageFormat::Png)
            .unwrap()
            .to_rgb8();
        let total_error = source
            .pixels()
            .zip(optimized.pixels())
            .map(|(original, reduced)| {
                (0..3)
                    .map(|channel| u64::from(original[channel].abs_diff(reduced[channel])))
                    .sum::<u64>()
            })
            .sum::<u64>();
        let mean_absolute_error = total_error as f64 / (128.0 * 64.0 * 3.0);
        let reader = png::Decoder::new(Cursor::new(&encoded))
            .read_info()
            .unwrap();

        assert!(
            mean_absolute_error < 18.0,
            "mean error was {mean_absolute_error}"
        );
        assert_eq!(reader.info().color_type, png::ColorType::Indexed);
    }

    #[test]
    fn cancellation_removes_any_temporary_output() {
        let dir = tempdir().unwrap();
        let input = dir.path().join("cancel.png");
        make_png(&input);
        let cancellation = CancellationToken::new();
        cancellation.cancel();
        assert!(
            compress_image(
                &job(&input),
                &CompressionOptions::default(),
                &cancellation,
                || {}
            )
            .is_err()
        );
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn cancellation_during_validation_never_commits_output() {
        let dir = tempdir().unwrap();
        let input = dir.path().join("validation-cancel.png");
        make_png(&input);
        let cancellation = CancellationToken::new();
        let result = compress_image(
            &job(&input),
            &CompressionOptions {
                min_savings_percent: 0,
                ..Default::default()
            },
            &cancellation,
            || cancellation.cancel(),
        );
        assert!(matches!(result, Err(super::CompressionError::Cancelled)));
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn below_savings_threshold_is_skipped_and_temp_file_is_removed() {
        let dir = tempdir().unwrap();
        let input = dir.path().join("threshold.png");
        make_png(&input);
        let original = fs::read(&input).unwrap();
        let options = CompressionOptions {
            min_savings_percent: 101,
            ..Default::default()
        };

        let result =
            compress_image(&job(&input), &options, &CancellationToken::new(), || {}).unwrap();

        assert_eq!(result.status, JobStatus::Skipped);
        assert!(result.skip_reason.as_deref().unwrap().contains("門檻"));
        assert!(result.output_path.is_none());
        assert_eq!(fs::read(&input).unwrap(), original);
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn jpeg_quality_mode_reencodes_without_changing_source() {
        let dir = tempdir().unwrap();
        let input = dir.path().join("original.jpg");
        fs::copy(
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/oriented-noise.jpg"
            ),
            &input,
        )
        .unwrap();
        let original = fs::read(&input).unwrap();

        let result = compress_image(
            &job(&input),
            &CompressionOptions {
                min_savings_percent: 0,
                ..Default::default()
            },
            &CancellationToken::new(),
            || {},
        )
        .unwrap();

        assert_eq!(result.status, JobStatus::Completed);
        assert_eq!(fs::read(input).unwrap(), original);
    }

    #[test]
    fn webp_quality_mode_keeps_webp_format_and_alpha() {
        let dir = tempdir().unwrap();
        let input = dir.path().join("transparent.webp");
        let pixels = RgbaImage::from_fn(512, 512, |x, y| {
            let seed = x.wrapping_mul(73_856_093) ^ y.wrapping_mul(19_349_663);
            Rgba([
                seed as u8,
                (seed >> 8) as u8,
                (seed >> 16) as u8,
                if x < 256 { 255 } else { 96 },
            ])
        });
        let mut encoded = Cursor::new(Vec::new());
        image::codecs::webp::WebPEncoder::new_lossless(&mut encoded)
            .write_image(
                &pixels.clone().into_raw(),
                512,
                512,
                image::ExtendedColorType::Rgba8,
            )
            .unwrap();
        fs::write(&input, encoded.into_inner()).unwrap();

        let result = compress_image(
            &job(&input),
            &CompressionOptions {
                min_savings_percent: 0,
                ..Default::default()
            },
            &CancellationToken::new(),
            || {},
        )
        .unwrap();

        let output = result
            .output_path
            .as_ref()
            .unwrap_or_else(|| panic!("WebP should produce a compressed output, got {result:?}"));
        let image = image::ImageReader::open(output).unwrap().decode().unwrap();
        assert_eq!(image.dimensions(), (512, 512));
        assert!(image.color().has_alpha());
    }

    #[test]
    fn apng_is_not_flattened_until_animation_preserving_optimization_exists() {
        let dir = tempdir().unwrap();
        let input = dir.path().join("motion.png");
        let mut encoder = png::Encoder::new(fs::File::create(&input).unwrap(), 2, 2);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_animated(2, 0).unwrap();
        let mut writer = encoder.write_header().unwrap();
        writer
            .write_image_data(&[255u8, 0, 0, 255].repeat(4))
            .unwrap();
        writer
            .write_image_data(&[0u8, 0, 255, 255].repeat(4))
            .unwrap();
        writer.finish().unwrap();
        let source = fs::read(&input).unwrap();

        let result = compress_image(
            &job(&input),
            &CompressionOptions::default(),
            &CancellationToken::new(),
            || {},
        );
        assert!(
            matches!(&result, Err(super::CompressionError::Codec(message)) if message.contains("APNG")),
            "unexpected APNG result: {result:?}"
        );
        assert_eq!(fs::read(&input).unwrap(), source);
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn jpeg_quality_mode_keeps_exif_date_and_source_bytes() {
        let dir = tempdir().unwrap();
        let input = dir.path().join("旅行 & $image.jpg");
        fs::copy(
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/oriented-noise.jpg"
            ),
            &input,
        )
        .unwrap();
        let original = fs::read(&input).unwrap();
        let options = CompressionOptions {
            mode: CompressionMode::Quality,
            min_savings_percent: 0,
            ..Default::default()
        };

        let result =
            compress_image(&job(&input), &options, &CancellationToken::new(), || {}).unwrap();

        assert_eq!(fs::read(&input).unwrap(), original);
        let output = result
            .output_path
            .expect("JPEG quality compression should reduce the fixture");
        let encoded = fs::read(output).unwrap();
        assert!(
            encoded
                .windows(b"2026:10:20 12:00:00".len())
                .any(|window| window == b"2026:10:20 12:00:00")
        );
    }
}
