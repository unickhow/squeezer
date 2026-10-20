use std::{path::Path, process::Command};

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoInfo {
    pub file_name: String,
    pub extension: String,
    pub width: u32,
    pub height: u32,
    pub duration_ms: u64,
    pub codec: String,
    pub audio_codec: Option<String>,
    pub thumbnail: String,
    pub original_bytes: u64,
}

pub fn analyze_video(path: &Path, helper: &Path) -> Result<VideoInfo, String> {
    let output = Command::new(helper).arg("analyze").arg(path).output()
        .map_err(|error| format!("無法啟動影片分析器：{error}"))?;
    if !output.status.success() {
        return Err(friendly_error(String::from_utf8_lossy(&output.stderr).trim()));
    }
    serde_json::from_slice(&output.stdout).map_err(|error| format!("影片分析結果無法讀取：{error}"))
}

#[cfg(test)]
mod tests {
    use super::{VideoInfo, friendly_error};

    #[test]
    fn video_info_deserializes_native_helper_metadata() {
        let info: VideoInfo = serde_json::from_str(r#"{"fileName":"clip.mov","extension":"mov","width":1920,"height":1080,"durationMs":1200,"codec":"avc1","audioCodec":"mp4a","thumbnail":"data:image/jpeg;base64,AA==","originalBytes":500000000}"#).unwrap();
        assert_eq!((info.width, info.height, info.duration_ms), (1920, 1080, 1200));
        assert_eq!(info.codec, "avc1");
        assert_eq!(info.audio_codec.as_deref(), Some("mp4a"));
        assert_eq!(info.original_bytes, 500_000_000);
    }

    #[test]
    fn native_reader_errors_are_presented_as_actionable_traditional_chinese() {
        assert_eq!(friendly_error("Cannot Open"), "無法讀取影片內容，請確認檔案未損毀且使用受支援的編碼。");
        assert_eq!(friendly_error("目前不支援多音訊軌影片"), "目前不支援多音訊軌影片");
    }
}

fn friendly_error(detail: &str) -> String {
    let normalized = detail.to_ascii_lowercase();
    if normalized.is_empty() || ["cannot open", "cannot decode", "invalid data", "failed to load"].iter().any(|phrase| normalized.contains(phrase)) {
        "無法讀取影片內容，請確認檔案未損毀且使用受支援的編碼。".into()
    } else { detail.to_owned() }
}
