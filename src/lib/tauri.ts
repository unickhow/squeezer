import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { AppPreferences, CompressionJob, CompressionOptions, VideoOptions } from "../types";

export const addFiles = (paths: string[]) => invoke<CompressionJob[]>("add_files", { paths });
export const startJobs = (options: CompressionOptions, overrides: Record<string, CompressionOptions>, videoOptions: VideoOptions, videoOverrides: Record<string, VideoOptions>, outputDirectory?: string) => invoke<void>("start_jobs", { options, overrides, videoOptions, videoOverrides, outputDirectory: outputDirectory ?? null });
export const loadPreferences = () => invoke<AppPreferences>("load_preferences");
export const savePreferences = (value: AppPreferences) => invoke<void>("save_preferences", { value });
export const selectOutputFolder = () => invoke<string | null>("select_output_folder");
export const videoPreviewPath = (id: string, after: boolean) => invoke<string>("video_preview_path", { id, after });
export const cancelJob = (id: string) => invoke<void>("cancel_job", { id });
export const retryJob = (id: string) => invoke<void>("retry_job", { id });
export const removeJob = (id: string) => invoke<void>("remove_job", { id });
export const clearJobs = () => invoke<void>("clear_jobs");
export const revealOutputs = (ids: string[]) => invoke<void>("reveal_outputs", { ids });
export const subscribeJobs = (onJob: (job: CompressionJob) => void) =>
  listen<CompressionJob>("job_updated", (event) => onJob(event.payload));
