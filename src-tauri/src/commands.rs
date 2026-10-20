use std::{collections::{HashMap, HashSet, VecDeque}, fs, path::{Path, PathBuf}, process::Command, sync::{Arc, Mutex}};

use tauri::{AppHandle, Emitter, Manager, State};
use tokio_util::sync::CancellationToken;

use crate::engine::{
    analyzer::{MediaInfo, analyze_media_with_image},
    images::{CompressionError, CompressionJob, CompressionOptions, JobStatus, compress_image},
    preview::{image_preview as render_image_preview, image_thumbnail_from_image},
};

#[derive(Default, Clone)]
pub struct JobManager(Arc<Mutex<JobState>>);

#[derive(Default)]
struct JobState {
    jobs: HashMap<String, CompressionJob>,
    cancellations: HashMap<String, CancellationToken>,
    analysis_queue: VecDeque<String>,
    analysis_worker_running: bool,
}

impl JobManager {
    fn remove(&self, id: &str) -> Result<(), String> {
        let mut state = self.0.lock().map_err(|_| "無法存取任務清單".to_string())?;
        if state.jobs.get(id).is_some_and(|job| is_active(job.status)) {
            return Err("請先取消進行中的工作".into());
        }
        state.jobs.remove(id).ok_or_else(|| "找不到此工作".to_string())?;
        state.cancellations.remove(id);
        Ok(())
    }

    fn clear(&self) -> Result<(), String> {
        let mut state = self.0.lock().map_err(|_| "無法存取任務清單".to_string())?;
        if state.jobs.values().any(|job| is_active(job.status)) {
            return Err("請先取消進行中的工作".into());
        }
        state.jobs.clear();
        state.cancellations.clear();
        Ok(())
    }
}

fn is_active(status: JobStatus) -> bool {
    matches!(status, JobStatus::Analyzing | JobStatus::Compressing | JobStatus::Validating)
}

fn progress_for_status(status: JobStatus) -> u8 {
    if status == JobStatus::Completed { 100 } else { 0 }
}

#[tauri::command]
pub fn select_files() -> Vec<String> {
    rfd::FileDialog::new()
        .add_filter("圖片", &["jpg", "jpeg", "png"])
        .pick_files()
        .unwrap_or_default()
        .into_iter()
        .map(|path| path.to_string_lossy().into_owned())
        .collect()
}

#[tauri::command]
pub fn add_files(app: AppHandle, manager: State<'_, JobManager>, paths: Vec<String>) -> Vec<CompressionJob> {
    let mut state = manager.0.lock().unwrap();
    let mut known_paths = state.jobs.values().map(|job| job.input_path.clone()).collect();
    let added = admit_paths(paths, &mut known_paths);
    for job in &added {
        if job.status == JobStatus::Analyzing {
            state.analysis_queue.push_back(job.id.clone());
            state.cancellations.insert(job.id.clone(), CancellationToken::new());
        }
        state.jobs.insert(job.id.clone(), job.clone());
        emit_update(&app, job);
    }
    let start_worker = claim_analysis_worker(&mut state);
    drop(state);
    if start_worker { run_analysis_worker(app, manager.inner().clone()); }
    added
}

fn admit_paths(paths: Vec<String>, known_paths: &mut HashSet<String>) -> Vec<CompressionJob> {
    paths.into_iter().filter_map(|path| {
        if !known_paths.insert(path.clone()) { return None; }
        Some(admit_file(path))
    }).collect()
}

fn admit_file(input_path: String) -> CompressionJob {
    let path = Path::new(&input_path);
    let admission = fs::metadata(path).map_err(|error| format!("找不到檔案或無法讀取：{error}"))
        .and_then(|metadata| {
            if !metadata.is_file() { return Err("只支援 JPEG 與 PNG 圖片".into()); }
            match path.extension().and_then(|value| value.to_str()).map(str::to_ascii_lowercase).as_deref() {
                Some("jpg" | "jpeg" | "png") => Ok(metadata.len()),
                _ => Err("只支援 JPEG 與 PNG 圖片".into()),
            }
        });
    match admission {
        Ok(bytes) => pending_job(input_path, bytes),
        Err(error) => new_job(input_path, None, Some(error)),
    }
}

fn pending_job(input_path: String, original_bytes: u64) -> CompressionJob {
    let mut job = new_job(input_path, None, None);
    job.status = JobStatus::Analyzing;
    job.original_bytes = original_bytes;
    job
}

fn apply_analysis(job: &mut CompressionJob, result: Result<MediaInfo, String>) {
    match result {
        Ok(info) => {
            job.file_name = info.file_name;
            job.extension = info.extension;
            job.width = info.width;
            job.height = info.height;
            job.original_bytes = info.original_bytes;
            job.status = JobStatus::Queued;
            job.error = None;
        }
        Err(error) => {
            job.status = JobStatus::Failed;
            job.error = Some(error);
        }
    }
}

fn claim_analysis_worker(state: &mut JobState) -> bool {
    if state.analysis_worker_running || state.analysis_queue.is_empty() { return false; }
    state.analysis_worker_running = true;
    true
}

fn run_analysis_worker(app: AppHandle, manager: JobManager) {
    std::thread::spawn(move || loop {
        let next = {
            let mut state = manager.0.lock().unwrap();
            match state.analysis_queue.pop_front() {
                Some(id) => state.jobs.get(&id).map(|job| (id, job.input_path.clone(), state.cancellations.get(&job.id).cloned())),
                None => { state.analysis_worker_running = false; return; }
            }
        };
        let Some((id, path, token)) = next else { continue; };
        let (result, thumbnail) = if token.as_ref().is_some_and(CancellationToken::is_cancelled) {
            (None, None)
        } else {
            match analyze_media_with_image(Path::new(&path)) {
                Ok((_info, _image)) if token.as_ref().is_some_and(CancellationToken::is_cancelled) => (None, None),
                Ok((info, image)) => {
                    let format = if info.extension == "png" { image::ImageFormat::Png } else { image::ImageFormat::Jpeg };
                    let thumbnail = image_thumbnail_from_image(&image, format).ok();
                    (Some(Ok(info)), thumbnail)
                }
                Err(error) => (Some(Err(error.to_string())), None),
            }
        };
        {
            let mut state = manager.0.lock().unwrap();
            let Some(job) = state.jobs.get_mut(&id) else { state.cancellations.remove(&id); continue; };
            if job.status != JobStatus::Analyzing {
                state.cancellations.remove(&id);
                continue;
            }
            if let Some(result) = result {
                if token.as_ref().is_some_and(CancellationToken::is_cancelled) {
                    job.status = JobStatus::Cancelled;
                    job.progress = 0;
                } else {
                    apply_analysis(job, result);
                    job.thumbnail = thumbnail;
                }
            } else {
                job.status = JobStatus::Cancelled;
                job.progress = 0;
            }
            let updated = job.clone();
            state.cancellations.remove(&id);
            emit_update(&app, &updated);
        }
    });
}

#[tauri::command]
pub fn image_preview(manager: State<'_, JobManager>, id: String, after: bool) -> Result<String, String> {
    let state = manager.0.lock().map_err(|_| "無法存取任務清單".to_string())?;
    let job = state.jobs.get(&id).ok_or_else(|| "找不到此圖片工作".to_string())?;
    let path = if after {
        job.output_path.as_deref().ok_or_else(|| "這個工作尚未產生輸出".to_string())?
    } else {
        &job.input_path
    };
    render_image_preview(Path::new(path)).map_err(|error| error.to_string())
}

#[tauri::command]
pub fn reveal_outputs(manager: State<'_, JobManager>, ids: Vec<String>) -> Result<(), String> {
    let paths = {
        let state = manager.0.lock().map_err(|_| "無法存取任務清單".to_string())?;
        collect_revealable_outputs(&state.jobs, &ids)?
    };
    let status = Command::new("/usr/bin/open").arg("-R").args(&paths).status()
        .map_err(|error| format!("無法在 Finder 顯示輸出檔案：{error}"))?;
    if !status.success() { return Err("Finder 無法顯示選取的輸出檔案".into()); }
    Ok(())
}

fn collect_revealable_outputs(jobs: &HashMap<String, CompressionJob>, ids: &[String]) -> Result<Vec<PathBuf>, String> {
    if ids.is_empty() { return Err("請選擇要顯示的輸出檔案".into()); }
    let paths = ids.iter().map(|id| {
        let job = jobs.get(id).ok_or_else(|| "找不到此圖片工作".to_string())?;
        if job.status != JobStatus::Completed { return Err("只有已完成的工作可以在 Finder 顯示".into()); }
        let output = job.output_path.as_deref().ok_or_else(|| "這個工作沒有輸出檔案".to_string())?;
        let path = PathBuf::from(output);
        if !path.is_file() { return Err("輸出檔案不存在或無法讀取".into()); }
        Ok(path)
    }).collect::<Result<Vec<_>, String>>()?;
    let parent = paths[0].parent().ok_or_else(|| "輸出路徑無效".to_string())?;
    if paths.iter().any(|path| path.parent() != Some(parent)) { return Err("一次只能顯示同一資料夾中的輸出".into()); }
    Ok(paths)
}

#[tauri::command]
pub fn start_jobs(app: AppHandle, manager: State<'_, JobManager>, options: CompressionOptions, overrides: HashMap<String, CompressionOptions>) {
    let mut state = manager.0.lock().unwrap();
    let queued = state.jobs.values().filter(|job| job.status == JobStatus::Queued).cloned().collect::<Vec<_>>();
    let batch = queued.into_iter().map(|mut job| {
        let token = CancellationToken::new();
        state.cancellations.insert(job.id.clone(), token.clone());
        job.status = JobStatus::Analyzing;
        job.progress = 0;
        state.jobs.insert(job.id.clone(), job.clone());
        emit_update(&app, &job);
        let job_options = options_for_job(&job.id, &options, &overrides);
        (job, token, job_options)
    }).collect::<Vec<_>>();
    drop(state);

    let app_for_batch = app.clone();
    std::thread::spawn(move || {
        for (mut running, token, job_options) in batch {
            if token.is_cancelled() {
                running.status = JobStatus::Cancelled;
                running.progress = 0;
                update_job(&app_for_batch, &running);
                continue;
            }
            running.status = JobStatus::Compressing;
            running.progress = 0;
            update_job(&app_for_batch, &running);
            let job_for_validation = running.clone();
            match compress_image(&running, &job_options, &token, || {
                let mut validating = job_for_validation.clone();
                validating.status = JobStatus::Validating;
                validating.progress = progress_for_status(validating.status);
                update_job(&app_for_batch, &validating);
            }) {
                Ok(result) => {
                    running.status = result.status;
                    running.progress = progress_for_status(result.status);
                    running.output_path = result.output_path.map(|path| path.to_string_lossy().into_owned());
                    running.output_bytes = result.output_bytes;
                    update_job(&app_for_batch, &running);
                }
                Err(CompressionError::Cancelled) => { running.status = JobStatus::Cancelled; running.progress = 0; update_job(&app_for_batch, &running); }
                Err(error) => { running.status = JobStatus::Failed; running.error = Some(error.to_string()); update_job(&app_for_batch, &running); }
            }
        }
    });
}

fn options_for_job(job_id: &str, defaults: &CompressionOptions, overrides: &HashMap<String, CompressionOptions>) -> CompressionOptions {
    overrides.get(job_id).unwrap_or(defaults).clone()
}

#[tauri::command]
pub fn cancel_job(app: AppHandle, manager: State<'_, JobManager>, id: String) -> Result<(), String> {
    let mut state = manager.0.lock().map_err(|_| "無法存取任務清單".to_string())?;
    if let Some(token) = state.cancellations.get(&id) { token.cancel(); }
    if let Some(job) = state.jobs.get_mut(&id) {
        if job.status == JobStatus::Queued {
            job.status = JobStatus::Cancelled;
            job.progress = 0;
            emit_update(&app, job);
        }
    }
    Ok(())
}

#[tauri::command]
pub fn remove_job(manager: State<'_, JobManager>, id: String) -> Result<(), String> {
    manager.remove(&id)
}

#[tauri::command]
pub fn clear_jobs(manager: State<'_, JobManager>) -> Result<(), String> {
    manager.clear()
}

fn new_job(input_path: String, info: Option<MediaInfo>, error: Option<String>) -> CompressionJob {
    let path = Path::new(&input_path);
    let file_name = info.as_ref().map(|info| info.file_name.clone())
        .or_else(|| path.file_name().map(|name| name.to_string_lossy().into_owned()))
        .unwrap_or_else(|| "未知檔案".into());
    let extension = info.as_ref().map(|info| info.extension.clone())
        .or_else(|| path.extension().map(|extension| extension.to_string_lossy().to_ascii_lowercase()))
        .unwrap_or_default();
    CompressionJob {
        id: uuid::Uuid::new_v4().to_string(),
        input_path,
        output_path: None,
        thumbnail: None,
        file_name,
        extension,
        media_type: "image".into(),
        width: info.as_ref().map(|info| info.width).unwrap_or_default(),
        height: info.as_ref().map(|info| info.height).unwrap_or_default(),
        status: if error.is_some() { JobStatus::Failed } else { JobStatus::Queued },
        progress: 0,
        original_bytes: info.map(|info| info.original_bytes).unwrap_or_default(),
        output_bytes: None,
        error,
    }
}

fn update_job(app: &AppHandle, job: &CompressionJob) {
    let mut emit = false;
    if let Some(manager) = app.try_state::<JobManager>() {
        let mut state = manager.0.lock().unwrap();
        emit = apply_job_update(&mut state, job);
    }
    if emit { emit_update(app, job); }
}

fn apply_job_update(state: &mut JobState, job: &CompressionJob) -> bool {
    let Some(current) = state.jobs.get(&job.id) else { return false; };
    if !is_active(current.status) && current.status != JobStatus::Queued && is_active(job.status) { return false; }
    state.jobs.insert(job.id.clone(), job.clone());
    if !is_active(job.status) { state.cancellations.remove(&job.id); }
    true
}

fn emit_update(app: &AppHandle, job: &CompressionJob) {
    let _ = app.emit("job_updated", job);
}

#[cfg(test)]
mod tests {
    use std::{collections::HashSet, fs};

    use tempfile::tempdir;

    use super::{JobManager, admit_paths, apply_analysis, apply_job_update, claim_analysis_worker, collect_revealable_outputs, new_job, options_for_job, pending_job, progress_for_status};
    use std::collections::HashMap;
    use crate::engine::{analyzer::MediaInfo, images::{CompressionMode, CompressionOptions, JobStatus}};

    #[test]
    fn queued_job_uses_override_when_present() {
        let defaults = CompressionOptions::default();
        let override_options = CompressionOptions { mode: CompressionMode::Small, png_lossy: true, ..Default::default() };
        let overrides = HashMap::from([("one".to_string(), override_options)]);
        let chosen = options_for_job("one", &defaults, &overrides);
        assert_eq!(chosen.mode, CompressionMode::Small);
        assert!(chosen.png_lossy);
    }

    #[test]
    fn queued_job_falls_back_to_batch_options() {
        let defaults = CompressionOptions { mode: CompressionMode::Balanced, png_lossy: true, ..Default::default() };
        let overrides = HashMap::from([("other".to_string(), CompressionOptions::default())]);
        let chosen = options_for_job("one", &defaults, &overrides);
        assert_eq!(chosen.mode, CompressionMode::Balanced);
        assert!(chosen.png_lossy);
    }

    #[test]
    fn invalid_image_becomes_failed_job_without_blocking_other_files() {
        let failed = new_job("broken.webp".into(), None, Some("只支援 JPEG 與 PNG 圖片".into()));
        let valid = new_job("good.jpg".into(), Some(MediaInfo { file_name: "good.jpg".into(), extension: "jpg".into(), width: 1, height: 1, original_bytes: 8 }), None);
        assert_eq!(failed.status, JobStatus::Failed);
        assert_eq!(valid.status, JobStatus::Queued);
    }

    #[test]
    fn queue_removal_rejects_active_jobs_and_clears_terminal_jobs() {
        let manager = JobManager::default();
        let mut job = new_job("photo.jpg".into(), None, Some("broken".into()));
        job.status = JobStatus::Compressing;
        let id = job.id.clone();
        manager.0.lock().unwrap().jobs.insert(id.clone(), job);

        assert!(manager.remove(&id).is_err());
        manager.0.lock().unwrap().jobs.get_mut(&id).unwrap().status = JobStatus::Failed;
        assert!(manager.remove(&id).is_ok());
        assert!(manager.0.lock().unwrap().jobs.is_empty());
    }

    #[test]
    fn stale_worker_update_cannot_resurrect_removed_or_terminal_job() {
        let mut state = super::JobState::default();
        let mut job = new_job("photo.jpg".into(), None, None);
        job.status = JobStatus::Cancelled;
        let id = job.id.clone();
        state.jobs.insert(id.clone(), job.clone());
        state.jobs.remove(&id);
        job.status = JobStatus::Compressing;
        assert!(!apply_job_update(&mut state, &job));
        assert!(!state.jobs.contains_key(&id));

        let mut terminal = new_job("other.jpg".into(), None, None);
        terminal.status = JobStatus::Cancelled;
        let terminal_id = terminal.id.clone();
        state.jobs.insert(terminal_id.clone(), terminal);
        job.id = terminal_id.clone();
        assert!(!apply_job_update(&mut state, &job));
        assert_eq!(state.jobs[&terminal_id].status, JobStatus::Cancelled);
    }

    #[test]
    fn admit_paths_keeps_valid_and_invalid_siblings() {
        let dir = tempdir().unwrap();
        let valid = dir.path().join("photo.jpg");
        fs::write(&valid, b"jpeg placeholder").unwrap();
        let unsupported = dir.path().join("notes.txt");
        fs::write(&unsupported, b"text").unwrap();
        let missing = dir.path().join("missing.png");
        let mut known = HashSet::new();

        let jobs = admit_paths(vec![valid.to_string_lossy().into(), unsupported.to_string_lossy().into(), missing.to_string_lossy().into(), valid.to_string_lossy().into()], &mut known);

        assert_eq!(jobs.len(), 3);
        assert_eq!(jobs[0].status, JobStatus::Analyzing);
        assert_eq!(jobs[0].original_bytes, 16);
        assert_eq!(jobs[1].status, JobStatus::Failed);
        assert!(jobs[1].error.as_deref().unwrap().contains("支援"));
        assert_eq!(jobs[2].status, JobStatus::Failed);
        assert!(jobs[2].error.is_some());
    }

    #[test]
    fn analysis_result_updates_one_job() {
        let mut job = pending_job("photo.jpg".into(), 10);
        apply_analysis(&mut job, Ok(MediaInfo { file_name: "photo.jpg".into(), extension: "jpg".into(), width: 12, height: 7, original_bytes: 10 }));
        assert_eq!(job.status, JobStatus::Queued);
        assert_eq!((job.width, job.height), (12, 7));
        assert_eq!(job.extension, "jpg");

        apply_analysis(&mut job, Err("圖片無法解碼".into()));
        assert_eq!(job.status, JobStatus::Failed);
        assert_eq!(job.error.as_deref(), Some("圖片無法解碼"));
    }

    #[test]
    fn analysis_worker_is_singleton_across_batches() {
        let mut state = super::JobState::default();
        assert!(!claim_analysis_worker(&mut state));
        state.analysis_queue.push_back("first".into());
        assert!(claim_analysis_worker(&mut state));
        state.analysis_queue.push_back("second".into());
        assert!(!claim_analysis_worker(&mut state));
    }

    #[test]
    fn progress_is_only_reported_when_known() {
        for status in [JobStatus::Queued, JobStatus::Analyzing, JobStatus::Compressing, JobStatus::Validating, JobStatus::Skipped, JobStatus::Failed, JobStatus::Cancelled] {
            assert_eq!(progress_for_status(status), 0, "{status:?}");
        }
        assert_eq!(progress_for_status(JobStatus::Completed), 100);
    }

    #[test]
    fn reveal_outputs_require_completed_files() {
        let dir = tempdir().unwrap();
        let a = dir.path().join("a.jpg");
        let b = dir.path().join("b.jpg");
        fs::write(&a, b"a").unwrap();
        fs::write(&b, b"b").unwrap();
        let mut first = new_job("source.jpg".into(), None, None);
        first.status = JobStatus::Completed;
        first.output_path = Some(a.to_string_lossy().into());
        let mut second = first.clone();
        second.id = "second".into();
        second.output_path = Some(b.to_string_lossy().into());
        let jobs = HashMap::from([(first.id.clone(), first.clone()), (second.id.clone(), second.clone())]);

        assert_eq!(collect_revealable_outputs(&jobs, &[first.id.clone()]).unwrap(), vec![a.clone()]);
        assert_eq!(collect_revealable_outputs(&jobs, &[first.id.clone(), second.id.clone()]).unwrap(), vec![a.clone(), b]);
        assert!(collect_revealable_outputs(&jobs, &[]).is_err());
        assert!(collect_revealable_outputs(&jobs, &["unknown".into()]).is_err());
        for status in [JobStatus::Queued, JobStatus::Skipped, JobStatus::Failed, JobStatus::Cancelled] {
            let mut non_completed = first.clone();
            non_completed.status = status;
            let non_completed_id = non_completed.id.clone();
            let jobs = HashMap::from([(non_completed_id.clone(), non_completed)]);
            assert!(collect_revealable_outputs(&jobs, &[non_completed_id]).is_err(), "{status:?}");
        }
        let mut no_output = first.clone();
        no_output.output_path = None;
        let jobs = HashMap::from([(no_output.id.clone(), no_output.clone())]);
        assert!(collect_revealable_outputs(&jobs, &[no_output.id]).is_err());
        let mut missing = first.clone();
        missing.output_path = Some(dir.path().join("missing.jpg").to_string_lossy().into());
        let jobs = HashMap::from([(missing.id.clone(), missing.clone())]);
        assert!(collect_revealable_outputs(&jobs, &[missing.id]).is_err());
        let other_dir = dir.path().join("other");
        fs::create_dir(&other_dir).unwrap();
        let other_file = other_dir.join("c.jpg");
        fs::write(&other_file, b"c").unwrap();
        let mut other = second;
        other.output_path = Some(other_file.to_string_lossy().into());
        let jobs = HashMap::from([(first.id.clone(), first.clone()), (other.id.clone(), other.clone())]);
        assert!(collect_revealable_outputs(&jobs, &[first.id.clone(), other.id]).is_err());
    }
}
