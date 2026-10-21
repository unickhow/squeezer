mod commands;
mod engine;

pub fn run() {
    jxl_oxide::integration::register_image_decoding_hook();
    tauri::Builder::default()
        .manage(commands::JobManager::default())
        .invoke_handler(tauri::generate_handler![
            commands::select_files,
            commands::add_files,
            commands::image_preview,
            commands::video_preview_path,
            commands::reveal_outputs,
            commands::start_jobs,
            commands::cancel_job,
            commands::retry_job,
            commands::remove_job,
            commands::clear_jobs,
        ])
        .run(tauri::generate_context!())
        .expect("failed to run Squeezer");
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use crate::engine::paths::output_path_for;

    #[test]
    fn output_path_is_non_overwriting_for_existing_destinations() {
        let dir = tempdir().unwrap();
        let input = dir.path().join("photo.jpg");
        fs::write(&input, b"source").unwrap();
        fs::write(dir.path().join("photo.squeezed.jpg"), b"existing").unwrap();

        let output = output_path_for(&input, dir.path()).unwrap();

        assert_eq!(output.file_name().unwrap(), "photo.squeezed-2.jpg");
        assert_eq!(fs::read(&input).unwrap(), b"source");
        assert_eq!(
            fs::read(output).unwrap_err().kind(),
            std::io::ErrorKind::NotFound
        );
    }

    #[test]
    fn output_path_keeps_unicode_source_name() {
        let dir = tempdir().unwrap();
        let input = dir.path().join("旅途照片.jpeg");
        fs::write(&input, b"source").unwrap();

        let output = output_path_for(&input, dir.path()).unwrap();

        assert_eq!(output.file_name().unwrap(), "旅途照片.squeezed.jpeg");
    }
}
