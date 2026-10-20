export type CompressionMode = "lossless" | "quality" | "balanced" | "small";
export type JobStatus = "queued" | "analyzing" | "compressing" | "validating" | "completed" | "skipped" | "failed" | "cancelled";

export interface CompressionJob {
  id: string;
  inputPath: string;
  outputPath?: string;
  thumbnail?: string;
  fileName: string;
  extension: "jpg" | "jpeg" | "png";
  mediaType: "image";
  width: number;
  height: number;
  status: JobStatus;
  progress: number;
  originalBytes: number;
  outputBytes?: number;
  error?: string;
}

export interface CompressionOptions {
  mode: CompressionMode;
  outputFormat: "original";
  preserveMetadata: boolean;
  minSavingsPercent: number;
  pngLossy: boolean;
}
