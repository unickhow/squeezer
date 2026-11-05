//! Usage: cargo run --release --example image_bench -- file...
//! Runs the real compression pipeline for every mode and prints size, detail and time.
use std::{fs, path::Path, time::Instant};

use squeezer_lib::engine::{
    analyzer::analyze_media,
    images::{CompressionJob, CompressionMode, CompressionOptions, JobStatus, MediaType, compress_image_to},
};
use tokio_util::sync::CancellationToken;

fn main() {
    let out = std::env::temp_dir().join("squeezer-image-bench");
    for arg in std::env::args().skip(1) {
        let input = Path::new(&arg);
        let info = analyze_media(input).unwrap();
        println!("\n{}  {}x{}  {} KB", input.file_name().unwrap().to_string_lossy(), info.width, info.height, info.original_bytes / 1024);
        for (name, mode) in [("quality ", CompressionMode::Quality), ("balanced", CompressionMode::Balanced), ("small   ", CompressionMode::Small)] {
            let dir = out.join(name.trim());
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            let job = CompressionJob {
                id: "bench".into(), input_path: arg.clone(), output_path: None, thumbnail: None,
                file_name: info.file_name.clone(), extension: info.extension.clone(), media_type: MediaType::Image,
                width: info.width, height: info.height, duration_ms: None, codec: None, audio_codec: None,
                status: JobStatus::Queued, progress: 0, original_bytes: info.original_bytes, output_bytes: None,
                error: None, skip_reason: None, detail: None,
            };
            let options = CompressionOptions { mode, min_savings_percent: 0, ..Default::default() };
            let started = Instant::now();
            let result = compress_image_to(&job, &options, Some(&dir), &CancellationToken::new(), || {});
            match result {
                Ok(r) => println!("  {name} {:>7} KB ({:>3}%)  {:<8} {:.1}s  {}", r.output_bytes.unwrap_or(0) / 1024, 100 * r.output_bytes.unwrap_or(0) / info.original_bytes, r.detail.unwrap_or_default(), started.elapsed().as_secs_f32(), r.output_path.map(|p| p.display().to_string()).unwrap_or_default()),
                Err(e) => println!("  {name} error: {e}"),
            }
        }
    }
}
