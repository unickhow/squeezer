import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { CompressionJob, CompressionOptions } from "../types";

export const addFiles = (paths: string[]) => invoke<CompressionJob[]>("add_files", { paths });
export const startJobs = (options: CompressionOptions, overrides: Record<string, CompressionOptions>) => invoke<void>("start_jobs", { options, overrides });
export const cancelJob = (id: string) => invoke<void>("cancel_job", { id });
export const removeJob = (id: string) => invoke<void>("remove_job", { id });
export const clearJobs = () => invoke<void>("clear_jobs");
export const revealOutputs = (ids: string[]) => invoke<void>("reveal_outputs", { ids });
export const subscribeJobs = (onJob: (job: CompressionJob) => void) =>
  listen<CompressionJob>("job_updated", (event) => onJob(event.payload));
