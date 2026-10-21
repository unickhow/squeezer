use std::{fs, io::Write, path::Path};

use serde::{Deserialize, Serialize};

use crate::engine::{images::CompressionMode, videos::VideoQuality};

const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppPreferences {
    pub schema_version: u32,
    pub output_location: OutputLocation,
    pub image: ImagePreferences,
    pub video: VideoPreferences,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum OutputLocation {
    SourceFolder,
    FixedFolder { path: String },
    AskEachBatch,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImagePreferences {
    pub mode: CompressionMode,
    pub min_savings_percent: u8,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoPreferences {
    pub quality: VideoQuality,
    pub max_dimension: u32,
    pub target_bytes: Option<u64>,
    pub min_savings_percent: u8,
}

#[derive(Debug, thiserror::Error)]
pub enum PreferenceError {
    #[error("偏好設定格式無法讀取：{0}")]
    Json(#[from] serde_json::Error),
    #[error("偏好設定檔案操作失敗：{0}")]
    Io(#[from] std::io::Error),
    #[error("不支援的偏好設定版本：{0}")]
    UnsupportedVersion(u32),
    #[error("偏好設定無效：{0}")]
    Invalid(String),
}

impl AppPreferences {
    pub fn defaults() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            output_location: OutputLocation::SourceFolder,
            image: ImagePreferences {
                mode: CompressionMode::Quality,
                min_savings_percent: 10,
            },
            video: VideoPreferences {
                quality: VideoQuality::Balanced,
                max_dimension: 0,
                target_bytes: None,
                min_savings_percent: 10,
            },
        }
    }

    pub fn validate(&self) -> Result<(), PreferenceError> {
        if self.schema_version != SCHEMA_VERSION {
            return Err(PreferenceError::UnsupportedVersion(self.schema_version));
        }
        if self.image.min_savings_percent > 99 || self.video.min_savings_percent > 99 {
            return Err(PreferenceError::Invalid(
                "最低節省比例須介於 0% 到 99%".into(),
            ));
        }
        if self.video.target_bytes == Some(0) {
            return Err(PreferenceError::Invalid("影片目標容量必須大於 0".into()));
        }
        if !matches!(self.video.max_dimension, 0 | 720 | 1080) {
            return Err(PreferenceError::Invalid(
                "影片長邊解析度只能選原尺寸、720p 或 1080p".into(),
            ));
        }
        if matches!(&self.output_location, OutputLocation::FixedFolder { path } if path.trim().is_empty())
        {
            return Err(PreferenceError::Invalid("固定輸出資料夾不可為空".into()));
        }
        Ok(())
    }
}

pub fn load_preferences_from(path: &Path) -> Result<AppPreferences, PreferenceError> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(AppPreferences::defaults());
        }
        Err(error) => return Err(error.into()),
    };
    let document: serde_json::Value = serde_json::from_slice(&bytes)?;
    let version = document
        .get("schemaVersion")
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| PreferenceError::Invalid("缺少偏好設定版本".into()))?;
    if version != u64::from(SCHEMA_VERSION) {
        return Err(PreferenceError::UnsupportedVersion(version as u32));
    }
    let value: AppPreferences = serde_json::from_value(document)?;
    value.validate()?;
    Ok(value)
}

pub fn save_preferences_to(path: &Path, value: &AppPreferences) -> Result<(), PreferenceError> {
    value.validate()?;
    let parent = path
        .parent()
        .ok_or_else(|| PreferenceError::Invalid("偏好設定檔案缺少上層資料夾".into()))?;
    fs::create_dir_all(parent)?;
    let bytes = serde_json::to_vec_pretty(value)?;
    let temp = parent.join(format!(".preferences-{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| -> Result<(), PreferenceError> {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        fs::rename(&temp, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_recommended_values() {
        let value = AppPreferences::defaults();
        assert_eq!(value.output_location, OutputLocation::SourceFolder);
        assert_eq!(value.image.mode, CompressionMode::Quality);
        assert_eq!(value.image.min_savings_percent, 10);
        assert_eq!(value.video.quality, VideoQuality::Balanced);
        assert_eq!(value.video.max_dimension, 0);
        assert_eq!(value.video.target_bytes, None);
        assert_eq!(value.video.min_savings_percent, 10);
        assert_eq!(value.schema_version, 1);
    }

    #[test]
    fn serializes_versioned_tagged_output_locations() {
        let mut value = AppPreferences::defaults();
        value.output_location = OutputLocation::FixedFolder {
            path: "/tmp/out".into(),
        };
        let encoded = serde_json::to_value(&value).unwrap();
        assert_eq!(encoded["schemaVersion"], 1);
        assert_eq!(encoded["outputLocation"]["kind"], "fixedFolder");
        assert_eq!(encoded["outputLocation"]["path"], "/tmp/out");
        assert_eq!(
            serde_json::from_value::<AppPreferences>(encoded).unwrap(),
            value
        );
    }

    #[test]
    fn rejects_invalid_ranges_and_empty_custom_folder() {
        let mut value = AppPreferences::defaults();
        value.image.min_savings_percent = 100;
        assert!(value.validate().is_err());
        value = AppPreferences::defaults();
        value.video.min_savings_percent = 100;
        assert!(value.validate().is_err());
        value = AppPreferences::defaults();
        value.video.target_bytes = Some(0);
        assert!(value.validate().is_err());
        value = AppPreferences::defaults();
        value.video.max_dimension = 900;
        assert!(value.validate().is_err());
        value = AppPreferences::defaults();
        value.output_location = OutputLocation::FixedFolder { path: "  ".into() };
        assert!(value.validate().is_err());
    }

    #[test]
    fn missing_file_loads_defaults_and_saved_value_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("preferences.json");
        assert_eq!(
            load_preferences_from(&path).unwrap(),
            AppPreferences::defaults()
        );
        let mut value = AppPreferences::defaults();
        value.image.mode = CompressionMode::Small;
        save_preferences_to(&path, &value).unwrap();
        assert_eq!(load_preferences_from(&path).unwrap(), value);
        let original = fs::read(&path).unwrap();
        value.video.target_bytes = Some(0);
        assert!(save_preferences_to(&path, &value).is_err());
        assert_eq!(fs::read(&path).unwrap(), original);
    }

    #[test]
    fn malformed_or_unsupported_preferences_are_preserved() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("preferences.json");
        for original in [br"{".as_slice(), br#"{"schemaVersion":99}"#.as_slice()] {
            fs::write(&path, original).unwrap();
            assert!(load_preferences_from(&path).is_err());
            assert_eq!(fs::read(&path).unwrap(), original);
        }
    }
}
