use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
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
    /// Short encoder summary for the UI, e.g. `q78` or `256 色`.
    pub detail: Option<String>,
}

#[cfg(test)]
pub fn compress_image<F>(
    job: &CompressionJob,
    options: &CompressionOptions,
    cancel: &CancellationToken,
    on_validating: F,
) -> Result<CompressionResult, CompressionError>
where
    F: FnOnce(),
{
    compress_image_to(job, options, None, cancel, on_validating)
}

pub fn compress_image_to<F>(
    job: &CompressionJob,
    options: &CompressionOptions,
    output_directory: Option<&Path>,
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
    let output_dir =
        output_directory.unwrap_or_else(|| input.parent().unwrap_or_else(|| Path::new(".")));
    let final_path = output_path_for(input, output_dir)?;
    let temp_path = temporary_path(&final_path, &job.id);
    let _temp_guard = TempOutput(temp_path.clone());
    let detail: Option<String>;

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
        let indexed = quantize_png(&source, options.mode, cancel).map_err(|error| {
            if cancel.is_cancelled() {
                CompressionError::Cancelled
            } else {
                CompressionError::PngQuantize(error.to_string())
            }
        })?;
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
        detail = png_palette_size(&optimized).map(|colors| format!("{colors} 色"));
    } else if job.extension.eq_ignore_ascii_case("webp") {
        if fs::metadata(input)?.len() > super::analyzer::MAX_INPUT_BYTES {
            return Err(super::analyzer::AnalyzeError::ImageTooLarge.into());
        }
        let outcome = super::webp::compress_webp(input, &temp_path, options.mode, cancel)?;
        detail = Some(format!("q{}", outcome.quality));
    } else if job.extension.eq_ignore_ascii_case("jpg")
        || job.extension.eq_ignore_ascii_case("jpeg")
    {
        let outcome = super::jpeg::compress_jpeg(
            input,
            &temp_path,
            options.mode,
            options.preserve_metadata,
            cancel,
        )?;
        detail = Some(format!("q{}", outcome.quality));
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
            detail: None,
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
        detail,
    })
}

/// Number of palette entries in an indexed PNG, if it is one.
fn png_palette_size(bytes: &[u8]) -> Option<usize> {
    let reader = png::Decoder::new(std::io::Cursor::new(bytes)).read_info().ok()?;
    reader.info().palette.as_ref().map(|palette| palette.len() / 3)
}

/// Palette sizes the adaptive search may choose from, largest first.
/// Largest per-channel error a pixel may have, and the share of pixels allowed to exceed it.
const COLOR_ERROR_LIMIT: u8 = 12;
const FAR_PIXEL_SHARE: f64 = 0.004;

/// Photographic content (many distinct colours) keeps at least this many palette entries:
/// below it, dithering grain and hue drift become visible even when scores look fine.
const PHOTO_UNIQUE_COLORS: usize = 30_000;
const PHOTO_PALETTE_FLOOR: u16 = 96;

const PALETTE_LADDER: [u16; 9] = [256, 192, 128, 96, 64, 48, 32, 24, 16];

/// Perceptual-similarity target per mode (see `similarity::blend`).
const fn png_target_score(mode: CompressionMode) -> f64 {
    match mode {
        CompressionMode::Quality => 0.96,
        CompressionMode::Balanced => 0.91,
        CompressionMode::Small => 0.89,
    }
}

type IndexedPixels = (Vec<[u8; 4]>, Vec<u8>);

fn quantize_png(
    source: &[u8],
    mode: CompressionMode,
    cancel: &CancellationToken,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let image = image::load_from_memory_with_format(source, image::ImageFormat::Png)?.to_rgba8();
    let opaque = image.pixels().all(|pixel| pixel[3] == 255);
    let probe = probe_image(&image);
    let colors = if probe.width() < 32 || probe.height() < 32 {
        PALETTE_LADDER[0]
    } else {
        search_palette_size(&probe, opaque, mode, cancel)?
    };
    let (palette, indices) = quantize_with(&image, opaque, colors)?;
    let indexed = encode_indexed_png(image.width(), image.height(), &palette, &indices)?;
    Ok(copy_color_chunks(source, indexed))
}

/// Binary-searches the smallest palette whose dithered result stays within the mode's allowed
/// loss, measured on a subsampled copy so large images stay fast. The allowed loss is relative
/// to the best a 256-colour palette can do for this image (photographs cannot reach a fixed
/// score with 256 colours), and capped by an absolute target for graphics that quantize cleanly.
fn search_palette_size(
    probe: &image::RgbaImage,
    opaque: bool,
    mode: CompressionMode,
    cancel: &CancellationToken,
) -> Result<u16, Box<dyn std::error::Error>> {
    let (reference, width, height) = similarity_planes(probe, opaque);
    let score_at = |colors: u16| -> Result<f64, Box<dyn std::error::Error>> {
        let (palette, indices) = quantize_with(probe, opaque, colors)?;
        let candidate = image::RgbaImage::from_fn(probe.width(), probe.height(), |x, y| {
            image::Rgba(palette[usize::from(indices[(y * probe.width() + x) as usize])])
        });
        let (planes, _, _) = similarity_planes(&candidate, opaque);
        let mut scores = Vec::new();
        for (a, b) in reference.iter().zip(&planes) {
            super::similarity::window_scores(a, b, width, height, &mut scores);
        }
        let score = super::similarity::blend(scores);
        // SSIM barely reacts to hue shifts, so a small photo inside a screenshot could turn
        // a different colour without lowering the score. Treat the palette as too coarse when
        // more than a sliver of pixels is far from its original colour.
        // Compared on the 2x2-averaged planes so dithering grain is not counted as colour error.
        let far = (0..reference[0].len())
            .filter(|&index| {
                reference
                    .iter()
                    .zip(&planes)
                    .take(3)
                    .any(|(a, b)| a[index].abs_diff(b[index]) > COLOR_ERROR_LIMIT)
            })
            .count();
        if far as f64 > reference[0].len() as f64 * FAR_PIXEL_SHARE {
            return Ok(0.0);
        }
        Ok(score)
    };

    let base = score_at(PALETTE_LADDER[0])?;
    let allowed_loss = match mode {
        CompressionMode::Quality => 0.02,
        CompressionMode::Balanced => 0.05,
        CompressionMode::Small => 0.07,
    };
    let target = png_target_score(mode).min(base - allowed_loss);
    let unique = probe.pixels().map(|p| p.0).collect::<std::collections::HashSet<_>>().len();
    let floor = if unique > PHOTO_UNIQUE_COLORS { PHOTO_PALETTE_FLOOR } else { *PALETTE_LADDER.last().unwrap() };
    let (mut low, mut high) = (1i32, PALETTE_LADDER.iter().rposition(|c| *c >= floor).unwrap_or(1) as i32);
    let mut best = 0usize;
    while low <= high {
        if cancel.is_cancelled() {
            return Err("cancelled".into());
        }
        let mid = (low + high) / 2;
        if score_at(PALETTE_LADDER[mid as usize])? >= target {
            best = mid as usize;
            low = mid + 1;
        } else {
            high = mid - 1;
        }
    }
    Ok(PALETTE_LADDER[best])
}

/// Every n-th pixel in both directions so the probe has about 1.2 megapixels at most.
fn probe_image(image: &image::RgbaImage) -> image::RgbaImage {
    let pixels = f64::from(image.width()) * f64::from(image.height());
    let step = (pixels / 1_200_000.0).sqrt().ceil().max(1.0) as u32;
    if step == 1 {
        return image.clone();
    }
    image::RgbaImage::from_fn(image.width() / step, image.height() / step, |x, y| {
        *image.get_pixel(x * step, y * step)
    })
}

/// 2×2 box-averaged channel planes (premultiplied RGB, plus alpha when present). Averaging
/// first keeps dithering noise from being scored as damage.
fn similarity_planes(image: &image::RgbaImage, opaque: bool) -> (Vec<Vec<u8>>, usize, usize) {
    let (width, height) = ((image.width() / 2) as usize, (image.height() / 2) as usize);
    let channels = if opaque { 3 } else { 4 };
    let mut planes = vec![vec![0u8; width * height]; channels];
    for y in 0..height {
        for x in 0..width {
            let mut sums = [0u32; 4];
            for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                let pixel = image.get_pixel((x * 2 + dx) as u32, (y * 2 + dy) as u32).0;
                let alpha = u32::from(pixel[3]);
                for channel in 0..3 {
                    sums[channel] += if opaque {
                        u32::from(pixel[channel])
                    } else {
                        u32::from(pixel[channel]) * alpha / 255
                    };
                }
                sums[3] += alpha;
            }
            for channel in 0..channels {
                planes[channel][y * width + x] = (sums[channel] / 4) as u8;
            }
        }
    }
    (planes, width, height)
}

fn quantize_with(
    image: &image::RgbaImage,
    opaque: bool,
    colors: u16,
) -> Result<IndexedPixels, Box<dyn std::error::Error>> {
    if opaque {
        quantize_opaque_png(image, colors)
    } else {
        Ok(quantize_alpha_png(image, colors))
    }
}

/// Colours are handled premultiplied by alpha so invisible pixels never cost palette entries.
type Premultiplied = [f32; 4];

fn premultiply(pixel: [u8; 4]) -> Premultiplied {
    let alpha = f32::from(pixel[3]);
    let scale = alpha / 255.0;
    [
        f32::from(pixel[0]) * scale,
        f32::from(pixel[1]) * scale,
        f32::from(pixel[2]) * scale,
        alpha,
    ]
}

fn unpremultiply(color: Premultiplied) -> [u8; 4] {
    let alpha = color[3].clamp(0.0, 255.0);
    if alpha < 0.5 {
        return [0, 0, 0, 0];
    }
    let scale = 255.0 / alpha;
    [
        (color[0] * scale).round().clamp(0.0, 255.0) as u8,
        (color[1] * scale).round().clamp(0.0, 255.0) as u8,
        (color[2] * scale).round().clamp(0.0, 255.0) as u8,
        alpha.round() as u8,
    ]
}

fn premultiplied_distance(a: Premultiplied, b: Premultiplied) -> f32 {
    let red = a[0] - b[0];
    let green = a[1] - b[1];
    let blue = a[2] - b[2];
    let alpha = a[3] - b[3];
    0.299 * red * red + 0.587 * green * green + 0.114 * blue * blue + 0.8 * alpha * alpha
}

fn nearest(palette: &[Premultiplied], color: Premultiplied) -> usize {
    let mut best = (0usize, f32::MAX);
    for (index, candidate) in palette.iter().enumerate() {
        let distance = premultiplied_distance(color, *candidate);
        if distance < best.1 {
            best = (index, distance);
        }
    }
    best.0
}

/// Median-cut on the exact colour histogram (6-6-6-4 bits, averaged inside each bin), followed by
/// a few weighted k-means passes, then error-diffused mapping.
fn quantize_alpha_png(image: &image::RgbaImage, colors: u16) -> IndexedPixels {
    use std::collections::HashMap;

    let mut bins: HashMap<u32, ([f64; 4], f64)> = HashMap::new();
    for pixel in image.pixels() {
        let key = (u32::from(pixel[0] >> 2) << 16)
            | (u32::from(pixel[1] >> 2) << 10)
            | (u32::from(pixel[2] >> 2) << 4)
            | u32::from(pixel[3] >> 4);
        let color = premultiply(pixel.0);
        let entry = bins.entry(key).or_insert(([0.0; 4], 0.0));
        for channel in 0..4 {
            entry.0[channel] += f64::from(color[channel]);
        }
        entry.1 += 1.0;
    }
    let entries: Vec<(Premultiplied, f32)> = bins
        .values()
        .map(|(sums, count)| {
            (
                std::array::from_fn(|channel| (sums[channel] / count) as f32),
                *count as f32,
            )
        })
        .collect();

    let weighted_mean = |members: &[usize]| -> Premultiplied {
        let mut sums = [0.0f64; 4];
        let mut total = 0.0f64;
        for index in members {
            let (color, weight) = entries[*index];
            total += f64::from(weight);
            for channel in 0..4 {
                sums[channel] += f64::from(color[channel]) * f64::from(weight);
            }
        }
        std::array::from_fn(|channel| (sums[channel] / total.max(1.0)) as f32)
    };

    let mut boxes = vec![(0..entries.len()).collect::<Vec<_>>()];
    while boxes.len() < usize::from(colors) {
        let candidate = boxes
            .iter()
            .enumerate()
            .filter(|(_, members)| members.len() > 1)
            .max_by(|(_, a), (_, b)| {
                box_spread(&entries, a).total_cmp(&box_spread(&entries, b))
            })
            .map(|(index, _)| index);
        let Some(candidate) = candidate else { break };
        let mut members = boxes.swap_remove(candidate);
        let channel = (0..4)
            .max_by(|a, b| {
                channel_range(&entries, &members, *a)
                    .total_cmp(&channel_range(&entries, &members, *b))
            })
            .unwrap_or(0);
        members.sort_by(|a, b| entries[*a].0[channel].total_cmp(&entries[*b].0[channel]));
        let total: f32 = members.iter().map(|index| entries[*index].1).sum();
        let mut accumulated = 0.0;
        let split = members
            .iter()
            .position(|index| {
                accumulated += entries[*index].1;
                accumulated >= total / 2.0
            })
            .unwrap_or(members.len() / 2)
            .clamp(1, members.len() - 1);
        let right = members.split_off(split);
        boxes.push(members);
        boxes.push(right);
    }
    let mut palette: Vec<Premultiplied> = boxes.iter().map(|members| weighted_mean(members)).collect();

    let passes = if entries.len() * palette.len() <= 60_000_000 { 4 } else { 1 };
    for _ in 0..passes {
        let mut groups = vec![Vec::new(); palette.len()];
        for (index, (color, _)) in entries.iter().enumerate() {
            groups[nearest(&palette, *color)].push(index);
        }
        for (slot, members) in groups.iter().enumerate() {
            if !members.is_empty() {
                palette[slot] = weighted_mean(members);
            }
        }
    }

    let indices = map_premultiplied(image, &palette, ALPHA_DITHER_STRENGTH);
    (palette.into_iter().map(unpremultiply).collect(), indices)
}

/// Slightly under full Floyd–Steinberg strength: avoids speckle on soft gradients and shadows.
const ALPHA_DITHER_STRENGTH: f32 = 0.75;

fn channel_range(entries: &[(Premultiplied, f32)], members: &[usize], channel: usize) -> f32 {
    let (low, high) = members
        .iter()
        .map(|index| entries[*index].0[channel])
        .fold((f32::MAX, f32::MIN), |(lo, hi), value| (lo.min(value), hi.max(value)));
    let weight: f32 = [0.299, 0.587, 0.114, 0.8][channel];
    (high - low) * weight.sqrt()
}

fn box_spread(entries: &[(Premultiplied, f32)], members: &[usize]) -> f32 {
    let total: f32 = members.iter().map(|index| entries[*index].1).sum();
    let range = (0..4)
        .map(|channel| channel_range(entries, members, channel))
        .fold(0.0, f32::max);
    total.sqrt() * range
}

fn quantize_opaque_png(
    image: &image::RgbaImage,
    colors: u16,
) -> Result<IndexedPixels, Box<dyn std::error::Error>> {
    use quantette::{ImageBuf, Pipeline, QuantizeMethod};

    let rgb = image::RgbImage::from_fn(image.width(), image.height(), |x, y| {
        let pixel = image.get_pixel(x, y);
        image::Rgb([pixel[0], pixel[1], pixel[2]])
    });
    let input = ImageBuf::try_from(rgb)?;
    let palette = Pipeline::new()
        .palette_size(colors.try_into()?)
        .quantize_method(QuantizeMethod::kmeans())
        .input_image(input.as_ref())
        .output_srgb8_palette();
    let palette = palette.ok_or("empty image")?;
    let palette: Vec<Premultiplied> = palette
        .iter()
        .map(|color| [f32::from(color.red), f32::from(color.green), f32::from(color.blue), 255.0])
        .collect();
    let strength = std::env::var("SQ_D").ok().and_then(|v| v.parse().ok()).unwrap_or(OPAQUE_DITHER_STRENGTH);
    let indices = map_premultiplied(image, &palette, strength);
    Ok((palette.into_iter().map(unpremultiply).collect(), indices))
}

/// Partial Floyd–Steinberg strength: full strength turns smooth gradients into visible grain
/// once the palette is small.
const OPAQUE_DITHER_STRENGTH: f32 = 0.75;

/// Chunks that define how the pixel values map to real colours. Dropping them makes viewers
/// assume sRGB, which shifts the tone of images saved with a display profile.
const COLOR_CHUNKS: [&[u8; 4]; 4] = [b"iCCP", b"sRGB", b"gAMA", b"cHRM"];

/// Copies the colour-space chunks from `source` into `indexed` right after its IHDR chunk.
fn copy_color_chunks(source: &[u8], indexed: Vec<u8>) -> Vec<u8> {
    const SIGNATURE: usize = 8;
    const IHDR_END: usize = SIGNATURE + 12 + 13;
    let mut chunks = Vec::new();
    let mut offset = SIGNATURE;
    while let Some(header) = source.get(offset..offset + 8) {
        let length = u32::from_be_bytes([header[0], header[1], header[2], header[3]]) as usize;
        let Some(end) = offset.checked_add(12).and_then(|v| v.checked_add(length)) else {
            break;
        };
        if end > source.len() {
            break;
        }
        let kind = &header[4..8];
        if kind == b"IDAT" {
            break;
        }
        if COLOR_CHUNKS.iter().any(|wanted| kind == &wanted[..]) {
            chunks.extend_from_slice(&source[offset..end]);
        }
        offset = end;
    }
    if chunks.is_empty() || indexed.len() < IHDR_END {
        return indexed;
    }
    let mut output = Vec::with_capacity(indexed.len() + chunks.len());
    output.extend_from_slice(&indexed[..IHDR_END]);
    output.extend_from_slice(&chunks);
    output.extend_from_slice(&indexed[IHDR_END..]);
    output
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

#[cfg(test)]
fn map_to_palette(image: &image::RgbaImage, palette: &[[u8; 4]], dither: bool) -> Vec<u8> {
    let palette: Vec<Premultiplied> = palette.iter().map(|color| premultiply(*color)).collect();
    map_premultiplied(image, &palette, if dither { 1.0 } else { 0.0 })
}

/// Nearest-colour mapping with Floyd–Steinberg error diffusion scaled by `strength` (0 = none).
fn map_premultiplied(
    image: &image::RgbaImage,
    palette: &[Premultiplied],
    strength: f32,
) -> Vec<u8> {
    let width = image.width() as usize;
    let height = image.height() as usize;
    let mut indices = Vec::with_capacity(width * height);
    let mut current_errors = vec![[0.0f32; 4]; width + 2];
    let mut next_errors = vec![[0.0f32; 4]; width + 2];

    for y in 0..height {
        for x in 0..width {
            let pixel = premultiply(image.get_pixel(x as u32, y as u32).0);
            let adjusted: Premultiplied = std::array::from_fn(|channel| {
                (pixel[channel] + current_errors[x + 1][channel]).clamp(0.0, 255.0)
            });
            let selected = nearest(palette, adjusted);
            indices.push(selected as u8);

            if strength > 0.0 {
                let chosen = palette[selected];
                for channel in 0..4 {
                    let error = (adjusted[channel] - chosen[channel]) * strength;
                    current_errors[x + 2][channel] += error * (7.0 / 16.0);
                    next_errors[x][channel] += error * (3.0 / 16.0);
                    next_errors[x + 1][channel] += error * (5.0 / 16.0);
                    next_errors[x + 2][channel] += error * (1.0 / 16.0);
                }
            }
        }
        std::mem::swap(&mut current_errors, &mut next_errors);
        next_errors.fill([0.0; 4]);
    }
    indices
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

fn temporary_path(output: &Path, job_id: &str) -> PathBuf {
    let stem = output.file_stem().unwrap_or_default().to_string_lossy();
    let extension = output.extension().unwrap_or_default().to_string_lossy();
    output.with_file_name(format!(".{stem}.{job_id}.tmp.{extension}"))
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
        compress_image_to,
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
    fn make_opaque_rgba_png(path: &std::path::Path) {
        let mut encoded = Cursor::new(Vec::new());
        PngEncoder::new(&mut encoded)
            .write_image(
                &RgbaImage::from_pixel(128, 96, Rgba([40, 120, 210, 255])).into_raw(),
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
            detail: None,
        }
    }

    #[test]
    fn writes_custom_output_to_selected_directory_without_overwriting() {
        let source_dir = tempdir().unwrap();
        let output_dir = tempdir().unwrap();
        let input = source_dir.path().join("photo.png");
        let existing = output_dir.path().join("photo.squeezed.png");
        make_png(&input);
        fs::write(&existing, b"keep existing output").unwrap();
        let original = fs::read(&input).unwrap();
        let result = compress_image_to(
            &job(&input),
            &CompressionOptions::default(),
            Some(output_dir.path()),
            &CancellationToken::new(),
            || {},
        )
        .unwrap();
        assert_eq!(result.status, JobStatus::Completed);
        let output = result.output_path.unwrap();
        assert_eq!(output.parent(), Some(output_dir.path()));
        assert_eq!(output.file_name().unwrap(), "photo.squeezed-2.png");
        assert_eq!(fs::read(&input).unwrap(), original);
        assert_eq!(fs::read(existing).unwrap(), b"keep existing output");
    }

    #[test]
    fn opaque_rgba_png_compression_may_drop_redundant_alpha_channel() {
        let source_dir = tempdir().unwrap();
        let output_dir = tempdir().unwrap();
        let input = source_dir.path().join("opaque-rgba.png");
        make_opaque_rgba_png(&input);
        let result = compress_image_to(
            &job(&input),
            &CompressionOptions::default(),
            Some(output_dir.path()),
            &CancellationToken::new(),
            || {},
        );
        assert!(result.is_ok(), "opaque RGBA PNG should compress: {result:?}");
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
        let (palette, indices) = super::quantize_opaque_png(&source, 256).unwrap();
        let encoded =
            super::encode_indexed_png(128, 64, &palette, &indices).unwrap();
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

    fn gradient_with_noise(alpha: bool) -> RgbaImage {
        RgbaImage::from_fn(192, 192, |x, y| {
            let noise = (x.wrapping_mul(73_856_093) ^ y.wrapping_mul(19_349_663)) % 9;
            Rgba([
                (x as u32 * 255 / 191 + noise).min(255) as u8,
                (y as u32 * 255 / 191 + noise).min(255) as u8,
                ((x + y) as u32 * 255 / 382).min(255) as u8,
                if alpha { 80 + (x as u32 * 175 / 191) as u8 } else { 255 },
            ])
        })
    }

    #[test]
    fn png_colour_space_chunks_survive_quantization() {
        let mut encoded = std::io::Cursor::new(Vec::new());
        let mut encoder = png::Encoder::new(&mut encoded, 64, 64);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_source_gamma(png::ScaledFloat::new(0.45455));
        let mut writer = encoder.write_header().unwrap();
        let pixels: Vec<u8> = (0..64 * 64).flat_map(|i| [(i % 64 * 4) as u8, 90, 160, 255]).collect();
        writer.write_image_data(&pixels).unwrap();
        writer.finish().unwrap();

        let output = super::quantize_png(
            &encoded.into_inner(),
            CompressionMode::Balanced,
            &CancellationToken::new(),
        )
        .unwrap();
        assert!(output.windows(4).any(|window| window == b"gAMA"));
        image::load_from_memory_with_format(&output, image::ImageFormat::Png).unwrap();
    }

    #[test]
    fn png_modes_never_choose_larger_palettes_when_looser() {
        for alpha in [false, true] {
            let source = gradient_with_noise(alpha);
            let mut sizes = Vec::new();
            for mode in [
                CompressionMode::Quality,
                CompressionMode::Balanced,
                CompressionMode::Small,
            ] {
                let mut encoded = Cursor::new(Vec::new());
                PngEncoder::new(&mut encoded)
                    .write_image(source.as_raw(), 192, 192, image::ExtendedColorType::Rgba8)
                    .unwrap();
                let indexed =
                    super::quantize_png(&encoded.into_inner(), mode, &CancellationToken::new())
                        .unwrap();
                let reader = png::Decoder::new(Cursor::new(&indexed)).read_info().unwrap();
                sizes.push(reader.info().palette.as_ref().unwrap().len() / 3);
            }
            assert!(
                sizes[0] >= sizes[1] && sizes[1] >= sizes[2],
                "alpha={alpha} palette sizes {sizes:?}"
            );
        }
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
