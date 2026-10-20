import type { CompressionJob } from "../types";

export function formatBytes(bytes?: number): string {
  if (bytes === undefined) return "—";
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(2)} MB`;
}

export function calculateSavings(jobs: CompressionJob[]): { bytes: number; percent: number } {
  const originalBytes = jobs.reduce((total, job) => total + job.originalBytes, 0);
  const savedBytes = jobs.reduce((total, job) => total + Math.max(0, job.originalBytes - (job.outputBytes ?? job.originalBytes)), 0);
  return { bytes: savedBytes, percent: originalBytes ? Math.round((savedBytes / originalBytes) * 100) : 0 };
}
