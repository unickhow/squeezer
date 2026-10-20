use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum JobStatus { Queued, Analyzing, Compressing, Validating, Completed, Skipped, Failed, Cancelled }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MediaType { Image, Video }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompressionJob {
    pub id: String,
    pub input_path: String,
    pub output_path: Option<String>,
    pub thumbnail: Option<String>,
    pub file_name: String,
    pub extension: String,
    pub media_type: MediaType,
    pub width: u32,
    pub height: u32,
    pub duration_ms: Option<u64>,
    pub codec: Option<String>,
    pub audio_codec: Option<String>,
    pub status: JobStatus,
    pub progress: u8,
    pub original_bytes: u64,
    pub output_bytes: Option<u64>,
    pub error: Option<String>,
    pub skip_reason: Option<String>,
}
