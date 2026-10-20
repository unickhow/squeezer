use std::{fs, io::{BufRead, BufReader}, path::{Path, PathBuf}, process::{Command, Stdio}, sync::mpsc, thread, time::Duration};

use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

use super::{images::{CompressionResult, JobStatus}, jobs::CompressionJob, paths::video_output_path_for, video_analysis::analyze_video};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum VideoQuality { High, Balanced, Small }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoOptions {
    pub quality: VideoQuality,
    pub max_dimension: u32,
    pub target_bytes: Option<u64>,
    pub min_savings_percent: u8,
}

impl Default for VideoOptions {
    fn default() -> Self { Self { quality: VideoQuality::Balanced, max_dimension: 0, target_bytes: None, min_savings_percent: 10 } }
}

struct TempOutput(PathBuf);
impl Drop for TempOutput { fn drop(&mut self) { let _ = fs::remove_file(&self.0); } }

pub fn compress_video<F, V>(job: &CompressionJob, options: &VideoOptions, helper: &Path, cancel: &CancellationToken, mut on_progress: F, on_validating: V) -> Result<CompressionResult, String>
where F: FnMut(u8), V: FnOnce() {
    if cancel.is_cancelled() { return Err("__cancelled__".into()); }
    if !matches!(options.max_dimension, 0 | 720 | 1080) { return Err("影片長邊解析度只能選原尺寸、720p 或 1080p".into()); }
    if options.target_bytes == Some(0) { return Err("目標容量必須大於 0".into()); }
    let input = Path::new(&job.input_path);
    let output_dir = input.parent().unwrap_or_else(|| Path::new("."));
    let final_path = video_output_path_for(input, output_dir).map_err(|error| error.to_string())?;
    let temp = temporary_path(&final_path, &job.id);
    let _cleanup = TempOutput(temp.clone());
    let duration_seconds = (job.duration_ms.unwrap_or(1) as f64 / 1000.0).max(0.1);
    let source_rate = (job.original_bytes as f64 * 8.0 / duration_seconds) as u64;
    let mut bitrate = if let Some(target_bytes) = options.target_bytes {
        ((target_bytes as f64 * 8.0 / duration_seconds) as u64).saturating_sub(if job.audio_codec.is_some() { 128_000 } else { 0 })
    } else {
        let ratio = match options.quality { VideoQuality::High => 0.75, VideoQuality::Balanced => 0.5, VideoQuality::Small => 0.32 };
        (source_rate as f64 * ratio) as u64
    }.clamp(100_000, 20_000_000) as usize;

    for attempt in 0..=usize::from(options.target_bytes.is_some()) {
        if cancel.is_cancelled() { return Err("__cancelled__".into()); }
        if temp.exists() { fs::remove_file(&temp).map_err(|error| error.to_string())?; }
        run_helper(helper, input, &temp, bitrate, options.max_dimension, cancel, &mut |progress| {
            if options.target_bytes.is_some() {
                on_progress(if attempt == 0 { progress * 45 / 95 } else { 50 + progress * 40 / 95 });
            } else { on_progress(progress); }
        })?;
        let actual_bytes = fs::metadata(&temp).map_err(|error| error.to_string())?.len();
        if let Some(target) = options.target_bytes {
            if actual_bytes > target && attempt == 0 {
                bitrate = ((bitrate as f64 * target as f64 / actual_bytes as f64) * 0.94).max(100_000.0) as usize;
                continue;
            }
        }
        break;
    }

    if cancel.is_cancelled() { return Err("__cancelled__".into()); }
    on_progress(95);
    on_validating();
    let output_info = analyze_video(&temp, helper)?;
    if output_info.codec != "avc1" || output_info.duration_ms.abs_diff(job.duration_ms.unwrap_or(output_info.duration_ms)) > 250 {
        return Err("輸出影片驗證失敗：格式或時長與來源不符".into());
    }
    if job.audio_codec.is_some() && output_info.audio_codec.is_none() { return Err("輸出影片驗證失敗：音訊軌未保留".into()); }
    if options.max_dimension > 0 && output_info.width.max(output_info.height) > options.max_dimension + 2 {
        return Err("輸出影片驗證失敗：解析度超過選取的長邊限制".into());
    }
    let output_bytes = fs::metadata(&temp).map_err(|error| error.to_string())?.len();
    let savings_percent = job.original_bytes.saturating_sub(output_bytes).saturating_mul(100) / job.original_bytes.max(1);
    if output_bytes >= job.original_bytes || savings_percent < u64::from(options.min_savings_percent) {
        return Ok(CompressionResult { status: JobStatus::Skipped, output_path: None, output_bytes: None, skip_reason: Some(format!("影片節省未達 {}% 門檻；未保留輸出檔。", options.min_savings_percent)) });
    }
    let committed = commit_without_overwrite(&temp, &final_path)?;
    let actual = fs::metadata(&committed).map_err(|error| error.to_string())?.len();
    let target_note = options.target_bytes.filter(|target| actual > *target).map(|target| format!("目標 {} bytes，實際 {} bytes；此素材無法達到目標。", target, actual));
    Ok(CompressionResult { status: JobStatus::Completed, output_path: Some(committed), output_bytes: Some(actual), skip_reason: target_note })
}

fn run_helper<F>(helper: &Path, input: &Path, output: &Path, bitrate: usize, max_dimension: u32, cancel: &CancellationToken, on_progress: &mut F) -> Result<(), String>
where F: FnMut(u8) {
    let mut child = Command::new(helper).args(["encode"]).arg(input).arg(output).arg(bitrate.to_string()).arg(max_dimension.to_string())
        .stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().map_err(|error| format!("無法啟動影片編碼器：{error}"))?;
    let stdout = child.stdout.take().ok_or("無法讀取影片編碼進度")?;
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || for line in BufReader::new(stdout).lines().flatten() { let _ = tx.send(line); });
    loop {
        if cancel.is_cancelled() { let _ = child.kill(); let _ = child.wait(); return Err("__cancelled__".into()); }
        while let Ok(line) = rx.try_recv() {
            if let Some(value) = line.strip_prefix("progress=").and_then(|value| value.parse::<f32>().ok()) {
                on_progress((value.clamp(0.0, 1.0) * 95.0).round() as u8);
            }
        }
        if let Some(status) = child.try_wait().map_err(|error| error.to_string())? {
            if status.success() { return Ok(()); }
            let output = child.wait_with_output().map_err(|error| error.to_string())?;
            return Err(String::from_utf8_lossy(&output.stderr).trim().to_owned());
        }
        thread::sleep(Duration::from_millis(40));
    }
}

fn temporary_path(output: &Path, job_id: &str) -> PathBuf {
    output.with_file_name(format!(".{}.{}.tmp.mp4", output.file_stem().unwrap_or_default().to_string_lossy(), job_id))
}

fn commit_without_overwrite(temp: &Path, output: &Path) -> Result<PathBuf, String> {
    let parent = output.parent().unwrap_or_else(|| Path::new("."));
    for suffix in 1..=1000 {
        let candidate = if suffix == 1 { output.to_path_buf() } else {
            let stem = output.file_stem().unwrap_or_default().to_string_lossy();
            parent.join(format!("{stem}-{suffix}.mp4"))
        };
        match fs::hard_link(temp, &candidate) {
            Ok(()) => { fs::remove_file(temp).map_err(|error| error.to_string())?; return Ok(candidate); }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.to_string()),
        }
    }
    Err("無法安全建立影片輸出檔".into())
}

#[cfg(test)]
mod tests {
    use std::{fs, os::unix::fs::PermissionsExt, thread, time::Duration};
    use tempfile::tempdir;
    use tokio_util::sync::CancellationToken;

    use crate::engine::{jobs::{CompressionJob, JobStatus, MediaType}, videos::{VideoOptions, VideoQuality, compress_video}};

    #[test]
    fn video_options_keep_quality_and_target_separate() {
        let options = VideoOptions { quality: VideoQuality::Small, max_dimension: 720, target_bytes: Some(2_000_000), min_savings_percent: 10 };
        let json = serde_json::to_value(options).unwrap();
        assert_eq!(json["quality"], "small");
        assert_eq!(json["maxDimension"], 720);
        assert_eq!(json["targetBytes"], 2_000_000);
    }

    #[test]
    fn cancellation_removes_partial_video_and_preserves_source_and_existing_output() {
        let dir = tempdir().unwrap();
        let input = dir.path().join("clip.mov");
        let existing = dir.path().join("clip.squeezed.mp4");
        fs::write(&input, b"original bytes").unwrap();
        fs::write(&existing, b"existing output").unwrap();
        let helper = dir.path().join("mock-helper.sh");
        fs::write(&helper, "#!/bin/sh\n[ \"$1\" = encode ] || exit 2\ndd if=/dev/zero of=\"$3\" bs=1024 count=8 2>/dev/null\necho progress=0.2\nsleep 5\n").unwrap();
        let mut permissions = fs::metadata(&helper).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&helper, permissions).unwrap();
        let job = CompressionJob {
            id: "cancel-test".into(), input_path: input.to_string_lossy().into(), output_path: None, thumbnail: None,
            file_name: "clip.mov".into(), extension: "mov".into(), media_type: MediaType::Video,
            width: 1920, height: 1080, duration_ms: Some(12_000), codec: Some("avc1".into()), audio_codec: None,
            status: JobStatus::Queued, progress: 0, original_bytes: 1_000_000, output_bytes: None, error: None, skip_reason: None,
        };
        let cancellation = CancellationToken::new();
        let cancel_after_start = cancellation.clone();
        let cancel_thread = thread::spawn(move || { thread::sleep(Duration::from_millis(150)); cancel_after_start.cancel(); });
        let result = compress_video(&job, &VideoOptions { quality: VideoQuality::Balanced, max_dimension: 0, target_bytes: None, min_savings_percent: 10 }, &helper, &cancellation, |_| {}, || {});
        cancel_thread.join().unwrap();
        assert_eq!(result.unwrap_err(), "__cancelled__");
        assert_eq!(fs::read(input).unwrap(), b"original bytes");
        assert_eq!(fs::read(existing).unwrap(), b"existing output");
        assert!(!fs::read_dir(dir.path()).unwrap().flatten().any(|entry| entry.file_name().to_string_lossy().contains("cancel-test.tmp")));
    }
}
