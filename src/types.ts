export type CompressionMode = "quality" | "balanced" | "small";
export type JobStatus = "queued" | "analyzing" | "compressing" | "validating" | "completed" | "skipped" | "failed" | "cancelled";
export type MediaType = "image" | "video";
export type VideoQuality = "high" | "balanced" | "small";

export interface CompressionJob {
  id: string;
  inputPath: string;
  outputPath?: string;
  thumbnail?: string;
  fileName: string;
  extension: string;
  mediaType: MediaType;
  width: number;
  height: number;
  durationMs?: number;
  codec?: string;
  audioCodec?: string;
  status: JobStatus;
  progress: number;
  originalBytes: number;
  outputBytes?: number;
  error?: string;
  skipReason?: string;
}

export interface CompressionOptions {
  mode: CompressionMode;
  outputFormat: "original";
  preserveMetadata: boolean;
  minSavingsPercent: number;
}

export interface VideoOptions {
  quality: VideoQuality;
  maxDimension: 0 | 1080 | 720;
  targetBytes?: number;
  minSavingsPercent: number;
}
