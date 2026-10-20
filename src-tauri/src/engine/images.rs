use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::Duration,
};

use tokio_util::sync::CancellationToken;

use super::{paths::output_path_for, validator::validate_image};

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CompressionMode { Lossless, Quality, Balanced, Small }

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompressionOptions {
    pub mode: CompressionMode,
    pub output_format: String,
    pub preserve_metadata: bool,
    pub min_savings_percent: u8,
    #[serde(default)]
    pub png_lossy: bool,
}

impl Default for CompressionOptions {
    fn default() -> Self {
        Self { mode: CompressionMode::Quality, output_format: "original".into(), preserve_metadata: true, min_savings_percent: 10, png_lossy: false }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum JobStatus { Queued, Analyzing, Compressing, Validating, Completed, Skipped, Failed, Cancelled }

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompressionJob {
    pub id: String,
    pub input_path: String,
    pub output_path: Option<String>,
    pub thumbnail: Option<String>,
    pub file_name: String,
    pub extension: String,
    pub media_type: String,
    pub width: u32,
    pub height: u32,
    pub status: JobStatus,
    pub progress: u8,
    pub original_bytes: u64,
    pub output_bytes: Option<u64>,
    pub error: Option<String>,
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
}

pub fn compress_image<F>(
    job: &CompressionJob,
    options: &CompressionOptions,
    cancel: &CancellationToken,
    on_validating: F,
) -> Result<CompressionResult, CompressionError>
where F: FnOnce() {
    if cancel.is_cancelled() { return Err(CompressionError::Cancelled); }
    let input = Path::new(&job.input_path);
    if job.extension == "jpeg" || job.extension == "jpg" {
        if options.mode == CompressionMode::Lossless {
            return Ok(CompressionResult { status: JobStatus::Skipped, output_path: None, output_bytes: None });
        }
    }

    let output_dir = input.parent().unwrap_or_else(|| Path::new("."));
    let final_path = output_path_for(input, output_dir)?;
    let temp_path = temporary_path(&final_path, &job.id);
    let _temp_guard = TempOutput(temp_path.clone());

    if job.extension == "png" {
        if fs::metadata(input)?.len() > super::analyzer::MAX_INPUT_BYTES {
            return Err(super::analyzer::AnalyzeError::ImageTooLarge.into());
        }
        let source = fs::read(input)?;
        let preset = match options.mode {
            CompressionMode::Lossless | CompressionMode::Quality => 2,
            CompressionMode::Balanced => 4,
            CompressionMode::Small => 6,
        };
        let optimized = if options.png_lossy {
            let indexed = quantize_png(&source).map_err(|error| CompressionError::PngQuantize(error.to_string()))?;
            oxipng::optimize_from_memory(&indexed, &oxipng::Options::from_preset(preset))?
        } else {
            oxipng::optimize_from_memory(&source, &oxipng::Options::from_preset(preset))?
        };
        if cancel.is_cancelled() { return Err(CompressionError::Cancelled); }
        let mut file = fs::OpenOptions::new().write(true).create_new(true).open(&temp_path)?;
        file.write_all(&optimized)?;
        file.sync_all()?;
    } else {
        let quality = match options.mode {
            CompressionMode::Quality => "90",
            CompressionMode::Balanced => "82",
            CompressionMode::Small => "68",
            CompressionMode::Lossless => unreachable!(),
        };
        run_sips(input, &temp_path, quality, cancel)?;
    }

    if cancel.is_cancelled() { return Err(CompressionError::Cancelled); }
    on_validating();
    let output_bytes = validate_image(input, &temp_path)?;
    if cancel.is_cancelled() { return Err(CompressionError::Cancelled); }
    let savings_percent = job.original_bytes.saturating_sub(output_bytes).saturating_mul(100)
        / job.original_bytes.max(1);
    if output_bytes >= job.original_bytes || savings_percent < u64::from(options.min_savings_percent) {
        return Ok(CompressionResult { status: JobStatus::Skipped, output_path: None, output_bytes: None });
    }

    let committed_path = commit_without_overwrite(&temp_path, input, output_dir)?;
    Ok(CompressionResult { status: JobStatus::Completed, output_path: Some(committed_path.clone()), output_bytes: Some(fs::metadata(committed_path)?.len()) })
}

fn quantize_png(source: &[u8]) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let image = image::load_from_memory_with_format(source, image::ImageFormat::Png)?.to_rgba8();
    let mut histogram = [0u64; 2048];
    for pixel in image.pixels() {
        histogram[palette_bin(pixel.0)] += 1;
    }
    let mut boxes = vec![(0usize..2048).filter(|index| histogram[*index] > 0).collect::<Vec<_>>()];
    while boxes.len() < 256 {
        let candidate = boxes.iter().enumerate().filter(|(_, values)| values.len() > 1)
            .max_by_key(|(_, values)| values.iter().map(|index| histogram[*index]).sum::<u64>())
            .map(|(index, _)| index);
        let Some(candidate) = candidate else { break };
        let mut values = boxes.remove(candidate);
        let channel = (0..4).max_by_key(|channel| {
            let (low, high) = values.iter().map(|index| bin_channels(*index)[*channel]).fold((u8::MAX, 0), |(lo, hi), value| (lo.min(value), hi.max(value)));
            high - low
        }).unwrap();
        values.sort_by_key(|index| bin_channels(*index)[channel]);
        let total: u64 = values.iter().map(|index| histogram[*index]).sum();
        let mut accumulated = 0;
        let split = values.iter().position(|index| { accumulated += histogram[*index]; accumulated >= total.div_ceil(2) }).unwrap_or(values.len() / 2).clamp(1, values.len() - 1);
        let right = values.split_off(split);
        boxes.push(values);
        boxes.push(right);
    }
    let palette: Vec<[u8; 4]> = boxes.iter().map(|values| {
        let mut sums = [0u64; 4];
        let mut total = 0u64;
        for index in values { let count = histogram[*index]; let channels = bin_channels(*index); total += count; for channel in 0..4 { sums[channel] += u64::from(channels[channel]) * count; } }
        std::array::from_fn(|channel| (sums[channel] / total) as u8)
    }).collect();
    let mut lookup = [[0u8; 256]; 8];
    let bins: Vec<[u8; 4]> = (0..2048).map(bin_channels).collect();
    for index in 0..2048 {
        let channels = bins[index];
        let nearest = palette.iter().enumerate().min_by_key(|(_, color)| channels.iter().zip(color.iter()).map(|(a,b)| { let delta = i32::from(*a)-i32::from(*b); delta*delta }).sum::<i32>()).unwrap().0 as u8;
        let key = index % 256;
        let band = index / 256;
        lookup[band][key] = nearest;
    }
    let indices: Vec<u8> = image.pixels().map(|pixel| { let index = palette_bin(pixel.0); lookup[index / 256][index % 256] }).collect();
    let mut output = Vec::new();
    let mut encoder = png::Encoder::new(&mut output, image.width(), image.height());
    encoder.set_color(png::ColorType::Indexed);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_palette(palette.iter().flat_map(|color| color[..3].iter().copied()).collect::<Vec<_>>());
    encoder.set_trns(palette.iter().map(|color| color[3]).collect::<Vec<_>>());
    let mut writer = encoder.write_header()?;
    writer.write_image_data(&indices)?;
    writer.finish()?;
    Ok(output)
}

fn palette_bin([red, green, blue, alpha]: [u8; 4]) -> usize {
    usize::from(red >> 5) * 256 + usize::from(green >> 5) * 32 + usize::from(blue >> 5) * 4 + usize::from(alpha >> 6)
}

fn bin_channels(index: usize) -> [u8; 4] {
    [(((index >> 8) & 7) * 255 / 7) as u8, (((index >> 5) & 7) * 255 / 7) as u8, (((index >> 2) & 7) * 255 / 7) as u8, ((index & 3) * 255 / 3) as u8]
}

fn temporary_path(output: &Path, job_id: &str) -> PathBuf {
    let stem = output.file_stem().unwrap_or_default().to_string_lossy();
    let extension = output.extension().unwrap_or_default().to_string_lossy();
    output.with_file_name(format!(".{stem}.{job_id}.tmp.{extension}"))
}

fn run_sips(input: &Path, output: &Path, quality: &str, cancel: &CancellationToken) -> Result<(), CompressionError> {
    let mut child = Command::new("/usr/bin/sips")
        .args(["-s", "format", "jpeg", "-s", "formatOptions", quality, "-o"])
        .arg(output).arg(input)
        .stdout(Stdio::null()).stderr(Stdio::piped()).spawn()
        .map_err(|error| CompressionError::Jpeg(format!("無法啟動 macOS ImageIO（sips）：{error}")))?;
    loop {
        if cancel.is_cancelled() {
            let _ = child.kill();
            let _ = child.wait();
            return Err(CompressionError::Cancelled);
        }
        if let Some(status) = child.try_wait()? {
            if status.success() { return Ok(()); }
            let stderr = child.wait_with_output().map(|output| String::from_utf8_lossy(&output.stderr).trim().to_owned()).unwrap_or_default();
            return Err(CompressionError::Jpeg(if stderr.is_empty() { "sips 無法編碼此圖片".into() } else { stderr }));
        }
        thread::sleep(Duration::from_millis(40));
    }
}

fn commit_without_overwrite(temp: &Path, input: &Path, output_dir: &Path) -> Result<PathBuf, CompressionError> {
    for _ in 0..100 {
        let candidate = output_path_for(input, output_dir)?;
        match fs::hard_link(temp, &candidate) {
            Ok(()) => { fs::remove_file(temp)?; return Ok(candidate); }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(CompressionError::Output(error)),
        }
    }
    Err(CompressionError::Jpeg("無法安全決定輸出檔名；請清理重複檔案後重試".into()))
}

struct TempOutput(PathBuf);
impl Drop for TempOutput { fn drop(&mut self) { let _ = fs::remove_file(&self.0); } }

#[cfg(test)]
mod tests {
    use std::{fs, io::Cursor};
    use image::{GenericImageView, ImageEncoder, Rgba, RgbaImage, codecs::png::PngEncoder};
    use tempfile::tempdir;
    use tokio_util::sync::CancellationToken;
    use super::{CompressionMode, CompressionOptions, CompressionJob, JobStatus, compress_image};

    fn make_png(path: &std::path::Path) {
        let mut encoded = Cursor::new(Vec::new());
        PngEncoder::new(&mut encoded).write_image(&RgbaImage::from_pixel(128, 96, Rgba([40, 120, 210, 128])).into_raw(), 128, 96, image::ExtendedColorType::Rgba8).unwrap();
        fs::write(path, encoded.into_inner()).unwrap();
    }
    fn job(path: &std::path::Path) -> CompressionJob {
        let info = super::super::analyzer::analyze_media(path).unwrap();
        CompressionJob { id: "test-job".into(), input_path: path.to_string_lossy().into(), output_path: None, thumbnail: None, file_name: info.file_name, extension: info.extension, media_type: "image".into(), width: info.width, height: info.height, status: JobStatus::Queued, progress: 0, original_bytes: info.original_bytes, output_bytes: None, error: None }
    }

    #[test]
    fn lossless_png_keeps_alpha_and_never_changes_source() {
        let dir = tempdir().unwrap();
        let input = dir.path().join("透明.png"); make_png(&input);
        let original = fs::read(&input).unwrap();
        let result = compress_image(&job(&input), &CompressionOptions { mode: CompressionMode::Lossless, ..Default::default() }, &CancellationToken::new(), || {}).unwrap();
        assert_eq!(fs::read(&input).unwrap(), original);
        if let Some(path) = result.output_path {
            let image = image::open(path).unwrap();
            assert!(image.color().has_alpha());
            assert_eq!(image.dimensions(), (128, 96));
        } else { assert_eq!(result.status, JobStatus::Skipped); }
        assert_eq!(result.status, JobStatus::Completed);
    }

    #[test]
    fn lossy_png_option_quantizes_colors_and_preserves_transparency() {
        let dir = tempdir().unwrap();
        let input = dir.path().join("colors.png");
        let mut pixels = RgbaImage::new(256, 128);
        for (x, y, pixel) in pixels.enumerate_pixels_mut() {
            *pixel = Rgba([x as u8, y as u8, (x.wrapping_mul(y) % 256) as u8, if x < 128 { 255 } else { 96 }]);
        }
        let mut encoded = Cursor::new(Vec::new());
        PngEncoder::new(&mut encoded).write_image(&pixels.into_raw(), 256, 128, image::ExtendedColorType::Rgba8).unwrap();
        fs::write(&input, encoded.into_inner()).unwrap();
        let original = fs::read(&input).unwrap();

        let options = CompressionOptions { png_lossy: true, min_savings_percent: 0, ..Default::default() };
        let result = compress_image(&job(&input), &options, &CancellationToken::new(), || {}).unwrap();

        assert_eq!(fs::read(&input).unwrap(), original);
        let output = result.output_path.expect("color reduction should make this fixture smaller");
        let bytes = fs::read(output).unwrap();
        let decoder = png::Decoder::new(Cursor::new(&bytes));
        let reader = decoder.read_info().unwrap();
        assert_eq!(reader.info().color_type, png::ColorType::Indexed);
        assert!(reader.info().trns.as_ref().is_some_and(|alpha| alpha.iter().any(|value| *value < 255)));
        assert!(bytes.len() < original.len());
    }

    #[test]
    fn cancellation_removes_any_temporary_output() {
        let dir = tempdir().unwrap(); let input = dir.path().join("cancel.png"); make_png(&input);
        let cancellation = CancellationToken::new(); cancellation.cancel();
        assert!(compress_image(&job(&input), &CompressionOptions::default(), &cancellation, || {}).is_err());
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn cancellation_during_validation_never_commits_output() {
        let dir = tempdir().unwrap();
        let input = dir.path().join("validation-cancel.png");
        make_png(&input);
        let cancellation = CancellationToken::new();
        let result = compress_image(&job(&input), &CompressionOptions { min_savings_percent: 0, ..Default::default() }, &cancellation, || cancellation.cancel());
        assert!(matches!(result, Err(super::CompressionError::Cancelled)));
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn below_savings_threshold_is_skipped_and_temp_file_is_removed() {
        let dir = tempdir().unwrap();
        let input = dir.path().join("threshold.png");
        make_png(&input);
        let original = fs::read(&input).unwrap();
        let options = CompressionOptions { min_savings_percent: 101, ..Default::default() };

        let result = compress_image(&job(&input), &options, &CancellationToken::new(), || {}).unwrap();

        assert_eq!(result.status, JobStatus::Skipped);
        assert!(result.output_path.is_none());
        assert_eq!(fs::read(&input).unwrap(), original);
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn jpeg_lossless_mode_skips_without_changing_source() {
        let dir = tempdir().unwrap();
        let input = dir.path().join("original.jpg");
        fs::copy(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/oriented-noise.jpg"), &input).unwrap();
        let original = fs::read(&input).unwrap();

        let result = compress_image(&job(&input), &CompressionOptions { mode: CompressionMode::Lossless, ..Default::default() }, &CancellationToken::new(), || {}).unwrap();

        assert_eq!(result.status, JobStatus::Skipped);
        assert_eq!(fs::read(input).unwrap(), original);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn jpeg_quality_mode_keeps_exif_date_and_source_bytes() {
        let dir = tempdir().unwrap();
        let input = dir.path().join("旅行 & $image.jpg");
        fs::copy(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/oriented-noise.jpg"), &input).unwrap();
        let original = fs::read(&input).unwrap();
        let options = CompressionOptions { mode: CompressionMode::Quality, min_savings_percent: 0, ..Default::default() };

        let result = compress_image(&job(&input), &options, &CancellationToken::new(), || {}).unwrap();

        assert_eq!(fs::read(&input).unwrap(), original);
        let output = result.output_path.expect("JPEG quality compression should reduce the fixture");
        let encoded = fs::read(output).unwrap();
        assert!(encoded.windows(b"2026:10:20 12:00:00".len()).any(|window| window == b"2026:10:20 12:00:00"));
    }
}
